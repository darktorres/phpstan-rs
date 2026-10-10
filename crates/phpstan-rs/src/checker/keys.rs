//! The keys a scope's facts are filed under.
//!
//! A scope narrows more than variables: a property chain (`$a->b->c`), an
//! array element named by a literal (`$a['k']`) or by a variable (`$a[$k]`),
//! a static property (`Foo::$p`), and the truthiness or `isset`-ness of any
//! of these. Each is a [`LocalKey`], a number made once per distinct
//! [`KeyData`] by a [`KeyTable`]: a key is as cheap to copy, compare and
//! order as a `u32`, and the table knows what it is built on, so rebinding
//! a variable finds what depends on it by following that, not by looking
//! through every key for a text that starts or contains the variable's.
//!
//! One table is shared by a function's scopes and everything forked from
//! them (`KeyTableRef`); a key means nothing to another table.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rustc_hash::FxHasher;
use syntax::ast::{BinOp, ExprId, ExprKind, ExprNode, Literal, NodeArenas, StaticTarget};
use syntax::name::{Name, NameKey};

/// What a scope's facts are filed under; see the module documentation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct LocalKey(u32);

/// What a [`LocalKey`] is.
#[derive(Clone, Debug)]
pub(crate) enum KeyData {
    /// `$name`.
    Var(Name),
    /// `base->prop`.
    Prop { base: LocalKey, prop: Name },
    /// `base[key]`, `key` a literal string or integer, as written.
    Index { base: LocalKey, key: Name },
    /// `base[$subscript]`: `subscript` is a variable.
    VarIndex { base: LocalKey, subscript: LocalKey },
    /// `class::$prop`. `class` is the class the name refers to - `self`
    /// is the class the scope belongs to - or `static` or `parent`, which
    /// stay what they are written as. A class is the same class in any
    /// case.
    StaticProp { class: Name, prop: Name },
    /// Whether the key's value is truthy.
    Truthy(LocalKey),
    /// Whether the key is set.
    Isset(LocalKey),
}

impl PartialEq for KeyData {
    fn eq(&self, other: &KeyData) -> bool {
        match (self, other) {
            (KeyData::Var(a), KeyData::Var(b)) => a == b,
            (
                KeyData::Prop { base, prop },
                KeyData::Prop {
                    base: other_base,
                    prop: other_prop,
                },
            ) => base == other_base && prop == other_prop,
            (
                KeyData::Index { base, key },
                KeyData::Index {
                    base: other_base,
                    key: other_key,
                },
            ) => base == other_base && key == other_key,
            (
                KeyData::VarIndex { base, subscript },
                KeyData::VarIndex {
                    base: other_base,
                    subscript: other_subscript,
                },
            ) => base == other_base && subscript == other_subscript,
            (
                KeyData::StaticProp { class, prop },
                KeyData::StaticProp {
                    class: other_class,
                    prop: other_prop,
                },
            ) => class.eq_ignore_case(other_class) && prop == other_prop,
            (KeyData::Truthy(a), KeyData::Truthy(b)) | (KeyData::Isset(a), KeyData::Isset(b)) => {
                a == b
            }
            _ => false,
        }
    }
}

impl Eq for KeyData {}

impl Hash for KeyData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // `hash_var` makes the first of these from text.
        match self {
            KeyData::Var(name) => {
                state.write_u8(0);
                state.write_u64(name.text_hash());
            }
            KeyData::Prop { base, prop } => {
                state.write_u8(1);
                state.write_u32(base.0);
                state.write_u64(prop.text_hash());
            }
            KeyData::Index { base, key } => {
                state.write_u8(2);
                state.write_u32(base.0);
                state.write_u64(key.text_hash());
            }
            KeyData::VarIndex { base, subscript } => {
                state.write_u8(3);
                state.write_u32(base.0);
                state.write_u32(subscript.0);
            }
            KeyData::StaticProp { class, prop } => {
                state.write_u8(4);
                state.write_u64(class.folded_hash());
                state.write_u64(prop.text_hash());
            }
            KeyData::Truthy(key) => {
                state.write_u8(5);
                state.write_u32(key.0);
            }
            KeyData::Isset(key) => {
                state.write_u8(6);
                state.write_u32(key.0);
            }
        }
    }
}

fn hash_of(data: &KeyData) -> u64 {
    let mut hasher = FxHasher::default();
    data.hash(&mut hasher);
    hasher.finish()
}

/// `hash_of(&KeyData::Var(name))` for a name's text hash, without a `Name`.
fn hash_var(text_hash: u64) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write_u8(0);
    hasher.write_u64(text_hash);
    hasher.finish()
}

/// One function's keys: made once, never removed.
#[derive(Default)]
pub(crate) struct KeyTable {
    data: Vec<KeyData>,
    index: hashbrown::HashTable<LocalKey>,
    // The keys made on each key (a base, a subscript, or the key a
    // truthiness or `isset` key tests), by the key's number.
    dependents: Vec<Vec<LocalKey>>,
}

/// A table shared by the scopes of one function.
pub(crate) type KeyTableRef = Rc<RefCell<KeyTable>>;

impl KeyTable {
    pub(crate) fn data(&self, key: LocalKey) -> &KeyData {
        &self.data[key.0 as usize]
    }

    /// The key of `data` when it has been made.
    pub(crate) fn find(&self, data: &KeyData) -> Option<LocalKey> {
        self.index
            .find(hash_of(data), |&k| self.data[k.0 as usize] == *data)
            .copied()
    }

    /// The key of the variable `name` when it has been made.
    pub(crate) fn find_var<N: NameKey + ?Sized>(&self, name: &N) -> Option<LocalKey> {
        self.index
            .find(hash_var(name.key_hash()), |&k| {
                matches!(&self.data[k.0 as usize], KeyData::Var(held) if name.is_key_of(held))
            })
            .copied()
    }

    /// The key of `data`, made when it has not been.
    pub(crate) fn intern(&mut self, data: KeyData) -> LocalKey {
        let hash = hash_of(&data);
        if let Some(&key) = self.index.find(hash, |&k| self.data[k.0 as usize] == data) {
            return key;
        }
        let key = LocalKey(u32::try_from(self.data.len()).expect("fewer than 2^32 keys"));
        match &data {
            KeyData::Var(_) | KeyData::StaticProp { .. } => {}
            KeyData::Prop { base, .. } | KeyData::Index { base, .. } => {
                self.dependents[base.0 as usize].push(key);
            }
            KeyData::VarIndex { base, subscript } => {
                self.dependents[base.0 as usize].push(key);
                self.dependents[subscript.0 as usize].push(key);
            }
            KeyData::Truthy(tested) | KeyData::Isset(tested) => {
                self.dependents[tested.0 as usize].push(key);
            }
        }
        self.data.push(data);
        self.dependents.push(Vec::new());
        let data = &self.data;
        self.index
            .insert_unique(hash, key, |&k| hash_of(&data[k.0 as usize]));
        key
    }

    /// The key of the variable `name`, made when it has not been.
    pub(crate) fn var(&mut self, name: &str) -> LocalKey {
        match self.find_var(name) {
            Some(key) => key,
            None => self.intern(KeyData::Var(Name::from(name))),
        }
    }

    /// Whether `key` is a variable.
    pub(crate) fn is_real_variable(&self, key: LocalKey) -> bool {
        matches!(self.data(key), KeyData::Var(_))
    }

    /// The keys built on `key`, directly.
    pub(crate) fn dependents(&self, key: LocalKey) -> &[LocalKey] {
        &self.dependents[key.0 as usize]
    }

    /// Whether `key` is `root` or built on it, through any number of steps.
    pub(crate) fn depends_on(&self, key: LocalKey, root: LocalKey) -> bool {
        if key == root {
            return true;
        }
        match self.data(key) {
            KeyData::Var(_) | KeyData::StaticProp { .. } => false,
            KeyData::Prop { base, .. } | KeyData::Index { base, .. } => {
                self.depends_on(*base, root)
            }
            KeyData::VarIndex { base, subscript } => {
                self.depends_on(*base, root) || self.depends_on(*subscript, root)
            }
            KeyData::Truthy(tested) | KeyData::Isset(tested) => self.depends_on(*tested, root),
        }
    }

    /// Every key built on `root`, through any number of steps, `root`
    /// itself not among them.
    pub(crate) fn all_dependents(&self, root: LocalKey) -> Vec<LocalKey> {
        let mut found = Vec::new();
        let mut stack = vec![root];
        while let Some(key) = stack.pop() {
            for &dependent in self.dependents(key) {
                if !found.contains(&dependent) {
                    found.push(dependent);
                    stack.push(dependent);
                }
            }
        }
        found
    }

    /// `key` as PHP source without its `$`s, as the keys were formatted
    /// before they were made by structure: `a`, `a->b`, `a[k]`, `a[$k]`,
    /// `Foo::$p`, `truthy(a)`, `isset(a)`.
    pub(crate) fn render(&self, key: LocalKey) -> String {
        let mut text = String::new();
        self.render_into(key, &mut text);
        text
    }

    fn render_into(&self, key: LocalKey, out: &mut String) {
        match self.data(key) {
            KeyData::Var(name) => out.push_str(name),
            KeyData::Prop { base, prop } => {
                self.render_into(*base, out);
                out.push_str("->");
                out.push_str(prop);
            }
            KeyData::Index { base, key } => {
                self.render_into(*base, out);
                out.push('[');
                out.push_str(key);
                out.push(']');
            }
            KeyData::VarIndex { base, subscript } => {
                self.render_into(*base, out);
                out.push_str("[$");
                self.render_into(*subscript, out);
                out.push(']');
            }
            KeyData::StaticProp { class, prop } => {
                out.push_str(class);
                out.push_str("::$");
                out.push_str(prop);
            }
            KeyData::Truthy(tested) => {
                out.push_str("truthy(");
                self.render_into(*tested, out);
                out.push(')');
            }
            KeyData::Isset(tested) => {
                out.push_str("isset(");
                self.render_into(*tested, out);
                out.push(')');
            }
        }
    }

    /// The key of the expression `e` narrows, made when it has not been:
    /// a variable, a property chain rooted in one, an array-index chain by
    /// literals or variables, a static property; looking through
    /// parentheses, an assignment (the key of its target) and `?? null`.
    pub(crate) fn key_of(
        &mut self,
        arenas: &NodeArenas,
        e: &ExprNode,
        class: Option<&Name>,
    ) -> Option<LocalKey> {
        self.resolve(arenas, e.id, class)
    }

    /// `key_of`, when the key has been made: a read never makes one.
    pub(crate) fn find_key(
        &self,
        arenas: &NodeArenas,
        e: &ExprNode,
        class: Option<&Name>,
    ) -> Option<LocalKey> {
        self.find_expr(arenas, e.id, class)
    }

    fn resolve(
        &mut self,
        arenas: &NodeArenas,
        id: ExprId,
        class: Option<&Name>,
    ) -> Option<LocalKey> {
        syntax::stack::guard(|| {
            let id = arenas.peeled(id)?;
            let e = arenas.expr(id)?;
            match &e.kind {
                ExprKind::Variable(name) => Some(self.var(name)),
                ExprKind::PropertyAccess(base, prop, _) => {
                    let base = self.resolve(arenas, *base, class)?;
                    Some(self.intern(KeyData::Prop {
                        base,
                        prop: prop.name.clone(),
                    }))
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Name(name), prop) => {
                    Some(self.intern(KeyData::StaticProp {
                        class: static_class(name, class),
                        prop: prop.name.clone(),
                    }))
                }
                ExprKind::Index(base, Some(index)) => {
                    let base = self.resolve(arenas, *base, class)?;
                    let index = arenas.expr(*index)?;
                    match crate::checker::literal_string_key(arenas, index) {
                        Some(text) => Some(self.intern(KeyData::Index {
                            base,
                            key: Name::from(text),
                        })),
                        None => match &index.kind {
                            ExprKind::Variable(subscript) => {
                                let subscript = self.var(subscript);
                                Some(self.intern(KeyData::VarIndex { base, subscript }))
                            }
                            _ => None,
                        },
                    }
                }
                ExprKind::Assign(target, _) | ExprKind::AssignRef(target, _) => {
                    self.resolve(arenas, *target, class)
                }
                ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
                    if arenas.expr(*rhs).is_some_and(|rhs| {
                        matches!(&rhs.kind, ExprKind::Literal(Literal::Null))
                    }) =>
                {
                    self.resolve(arenas, *lhs, class)
                }
                _ => None,
            }
        })
    }

    fn find_expr(&self, arenas: &NodeArenas, id: ExprId, class: Option<&Name>) -> Option<LocalKey> {
        syntax::stack::guard(|| {
            let id = arenas.peeled(id)?;
            let e = arenas.expr(id)?;
            match &e.kind {
                ExprKind::Variable(name) => self.find_var(name),
                ExprKind::PropertyAccess(base, prop, _) => {
                    let base = self.find_expr(arenas, *base, class)?;
                    self.find(&KeyData::Prop {
                        base,
                        prop: prop.name.clone(),
                    })
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Name(name), prop) => {
                    self.find(&KeyData::StaticProp {
                        class: static_class(name, class),
                        prop: prop.name.clone(),
                    })
                }
                ExprKind::Index(base, Some(index)) => {
                    let base = self.find_expr(arenas, *base, class)?;
                    let index = arenas.expr(*index)?;
                    match crate::checker::literal_string_key(arenas, index) {
                        Some(text) => self.find(&KeyData::Index {
                            base,
                            key: Name::from(text),
                        }),
                        None => match &index.kind {
                            ExprKind::Variable(subscript) => {
                                let subscript = self.find_var(subscript)?;
                                self.find(&KeyData::VarIndex { base, subscript })
                            }
                            _ => None,
                        },
                    }
                }
                ExprKind::Assign(target, _) | ExprKind::AssignRef(target, _) => {
                    self.find_expr(arenas, *target, class)
                }
                ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
                    if arenas.expr(*rhs).is_some_and(|rhs| {
                        matches!(&rhs.kind, ExprKind::Literal(Literal::Null))
                    }) =>
                {
                    self.find_expr(arenas, *lhs, class)
                }
                _ => None,
            }
        })
    }
}

/// The class a static property's `name` refers to: `self` is `scope`'s
/// class, `static` and `parent` stay what they are, a name is its
/// resolution.
pub(super) fn static_class(name: &syntax::ast::QualifiedName, scope: Option<&Name>) -> Name {
    if name.is_keyword("self") {
        return scope.cloned().unwrap_or_else(|| name.written.clone());
    }
    if name.is_keyword("static") || name.is_keyword("parent") {
        return Name::from(name.written.to_ascii_lowercase());
    }
    match name.resolved_class() {
        Some(class) => Name::from(class),
        None => name.written.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::{ExprId, NodeArenas};

    /// The keys as they were formatted before they were made by structure:
    /// what `KeyTable::render` of `key_of` has to equal.
    fn formatted(arenas: &NodeArenas, id: ExprId) -> Option<String> {
        let e = arenas.expr(arenas.peeled(id)?)?;
        match &e.kind {
            ExprKind::Variable(name) => Some(name.to_string()),
            ExprKind::PropertyAccess(base, prop, _) => {
                formatted(arenas, *base).map(|base_key| format!("{base_key}->{prop}"))
            }
            ExprKind::StaticPropertyFetch(StaticTarget::Name(class_name), name) => {
                let class = match class_name.resolved_class() {
                    Some(resolved) => resolved.to_string(),
                    None => class_name.to_string(),
                };
                Some(format!("{class}::${name}"))
            }
            ExprKind::Index(base, Some(idx)) => {
                let base_key = formatted(arenas, *base)?;
                let idx = arenas.expr(*idx)?;
                match crate::checker::literal_string_key(arenas, idx) {
                    Some(key) => Some(format!("{base_key}[{key}]")),
                    None => match &idx.kind {
                        ExprKind::Variable(subscript) => Some(format!("{base_key}[${subscript}]")),
                        _ => None,
                    },
                }
            }
            ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) => formatted(arenas, *lhs),
            ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
                if arenas
                    .expr(*rhs)
                    .is_some_and(|rhs| matches!(&rhs.kind, ExprKind::Literal(Literal::Null))) =>
            {
                formatted(arenas, *lhs)
            }
            _ => None,
        }
    }

    const SOURCE: &str = r#"<?php
{
    $a->b->c = 1;
    $a['k1']['k2'] = $o->p['x'];
    $a[$k][$b] = 1;
    $a[1] = $b[2][$k];
    if (is_string($a->b['x'] ?? null)) { Foo::$bar = $a; }
    self::$count++;
    ($x = $a->f()) !== false;
    $a->{$k} = $b[f()];
    $o?->p->q = $a['a b']['c'] ?? null;
    isset($a[$k]->z) && !empty(\Ns\Foo::$list[0]);
}
"#;

    #[test]
    fn a_key_made_by_structure_renders_as_the_key_formatted_by_text() {
        let parsed = syntax::parse(SOURCE.as_bytes()).expect("parses");
        struct Check {
            table: KeyTable,
            seen: usize,
        }
        impl syntax::visit::arena::Visitor for Check {
            fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
                let Some(e) = arenas.expr(id) else {
                    return;
                };
                let by_text = formatted(arenas, id);
                let made = self.table.key_of(arenas, e, None);
                assert_eq!(made.map(|k| self.table.render(k)), by_text);
                assert_eq!(self.table.find_key(arenas, e, None), made, "{by_text:?}");
                self.seen += usize::from(made.is_some());
                syntax::visit::arena::walk_expr(self, arenas, id);
            }
        }
        let mut check = Check {
            table: KeyTable::default(),
            seen: 0,
        };
        syntax::visit::arena::walk_stmts(&mut check, &parsed.arenas, parsed.statements());
        assert!(check.seen > 20, "{}", check.seen);
    }

    #[test]
    fn one_key_is_made_once_and_found_by_what_it_is() {
        let mut table = KeyTable::default();
        let a = table.var("a");
        assert_eq!(table.var("a"), a);
        assert_eq!(table.find_var("a"), Some(a));
        assert_eq!(table.find_var("b"), None);
        let prop = table.intern(KeyData::Prop {
            base: a,
            prop: "p".into(),
        });
        assert_eq!(
            table.find(&KeyData::Prop {
                base: a,
                prop: "p".into()
            }),
            Some(prop)
        );
        // A property and an index of one text are two keys.
        let index = table.intern(KeyData::Index {
            base: a,
            key: "p".into(),
        });
        assert_ne!(index, prop);
        assert!(table.is_real_variable(a));
        assert!(!table.is_real_variable(prop));
    }

    #[test]
    fn a_key_depends_on_what_it_is_built_on_at_any_depth() {
        let mut table = KeyTable::default();
        let (a, k, m) = (table.var("a"), table.var("k"), table.var("m"));
        let ap = table.intern(KeyData::Prop {
            base: a,
            prop: "p".into(),
        });
        let apq = table.intern(KeyData::Index {
            base: ap,
            key: "q".into(),
        });
        let mk = table.intern(KeyData::VarIndex {
            base: m,
            subscript: k,
        });
        let mkp = table.intern(KeyData::Prop {
            base: mk,
            prop: "p".into(),
        });
        assert!(table.depends_on(apq, a));
        assert!(table.depends_on(apq, ap));
        assert!(!table.depends_on(ap, apq));
        assert!(table.depends_on(mkp, k));
        assert!(table.depends_on(mkp, m));
        assert!(!table.depends_on(mkp, a));
        assert!(table.depends_on(a, a));
        let mut under_a = table.all_dependents(a);
        under_a.sort();
        assert_eq!(under_a, [ap, apq]);
        let mut under_k = table.all_dependents(k);
        under_k.sort();
        assert_eq!(under_k, [mk, mkp]);
    }

    #[test]
    fn text_that_looks_like_a_subscript_is_no_dependency() {
        // `$m['[$a]']` is keyed by a literal that reads like `[$a]`; it
        // does not depend on the variable `$a`.
        let mut table = KeyTable::default();
        let (a, m) = (table.var("a"), table.var("m"));
        let literal = table.intern(KeyData::Index {
            base: m,
            key: "[$a]".into(),
        });
        assert!(!table.depends_on(literal, a));
    }

    #[test]
    fn a_static_property_is_the_same_key_in_any_case_of_its_class() {
        let mut table = KeyTable::default();
        let a = table.intern(KeyData::StaticProp {
            class: "App\\Foo".into(),
            prop: "p".into(),
        });
        let b = table.intern(KeyData::StaticProp {
            class: "app\\foo".into(),
            prop: "p".into(),
        });
        assert_eq!(a, b);
        // A property name is exact.
        let c = table.intern(KeyData::StaticProp {
            class: "App\\Foo".into(),
            prop: "P".into(),
        });
        assert_ne!(a, c);
    }

    #[test]
    fn self_names_the_class_of_the_scope_and_static_stays_late() {
        use syntax::ast::QualifiedName;
        let scope = Name::from("Foo");
        let named = |written: &str| {
            let mut name = QualifiedName::new(written, syntax::span::Span::default());
            name.resolved = syntax::ast::ResolvedName::Exact(Name::from("Foo"));
            name
        };
        let keyword = |written: &str| QualifiedName::new(written, syntax::span::Span::default());
        let (this, foo) = (
            static_class(&keyword("self"), Some(&scope)),
            static_class(&named("Foo"), Some(&scope)),
        );
        assert!(this.eq_ignore_case(&foo));
        let late = static_class(&keyword("static"), Some(&scope));
        let parent = static_class(&keyword("PARENT"), Some(&scope));
        assert!(!late.eq_ignore_case(&foo));
        assert!(!late.eq_ignore_case(&parent));
        assert_eq!(parent, Name::from("parent"));
    }

    #[test]
    fn two_literal_subscripts_are_two_keys_where_one_text_was_both() {
        // `$a['k][j']` and `$a['k']['j']` both formatted as `a[k][j]`.
        let mut table = KeyTable::default();
        let a = table.var("a");
        let one = table.intern(KeyData::Index {
            base: a,
            key: "k][j".into(),
        });
        let k = table.intern(KeyData::Index {
            base: a,
            key: "k".into(),
        });
        let two = table.intern(KeyData::Index {
            base: k,
            key: "j".into(),
        });
        assert_eq!(table.render(one), table.render(two));
        assert_ne!(one, two);
    }
}
