//! What a file references, recorded by the parser as it resolves names and
//! reads doc comments, so no walk of the tree has to find them.

use std::hash::Hasher;
use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::ast::{QualifiedName, ResolvedName, UseKind};
use crate::name::Name;
use crate::name_resolution::NameContext;
use crate::parser::NameKind;

/// What a reference in a file is to, which a change to it makes the file
/// stale for: a class or an interface or a trait or an enum, a function, a
/// global constant, or a global variable (`global $name;`). Each kind has
/// the equality PHP gives it: class and function names ignore case, a
/// constant's namespace does and its own name does not, a variable's name
/// is exact. A name holds the hashes it was made with.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RefKey {
    Class(Name),
    Function(Name),
    Constant(Name),
    GlobalVar(Name),
}

impl RefKey {
    /// The class `name` (with or without its leading `\`) names.
    pub fn class(name: &Name) -> RefKey {
        RefKey::Class(without_backslash(name))
    }

    pub fn function(name: &Name) -> RefKey {
        RefKey::Function(without_backslash(name))
    }

    pub fn constant(name: &Name) -> RefKey {
        RefKey::Constant(without_backslash(name))
    }

    /// `class` for a text, which the key holds without sharing it.
    pub fn class_text(text: &str) -> RefKey {
        RefKey::Class(Name::from(text.trim_start_matches('\\')))
    }

    pub fn function_text(text: &str) -> RefKey {
        RefKey::Function(Name::from(text.trim_start_matches('\\')))
    }

    pub fn constant_text(text: &str) -> RefKey {
        RefKey::Constant(Name::from(text.trim_start_matches('\\')))
    }

    pub fn global_variable(name: &str) -> RefKey {
        RefKey::GlobalVar(Name::from(name))
    }

    /// The name the key holds.
    pub fn name(&self) -> &Name {
        match self {
            RefKey::Class(name)
            | RefKey::Function(name)
            | RefKey::Constant(name)
            | RefKey::GlobalVar(name) => name,
        }
    }

    /// The name the built-in stubs' own references to it are keyed by: in
    /// lower case, whatever the kind. A variable is in none of them.
    pub fn stub_key(&self) -> Option<&str> {
        match self {
            RefKey::GlobalVar(_) => None,
            other => Some(other.name().lower()),
        }
    }

    #[inline]
    fn tag(&self) -> u8 {
        match self {
            RefKey::Class(_) => 0,
            RefKey::Function(_) => 1,
            RefKey::Constant(_) => 2,
            RefKey::GlobalVar(_) => 3,
        }
    }
}

/// Sorts a file's distinct reference keys in a stable, hash-friendly order:
/// kind, the key's own hash, then its PHP-folded bytes. Equal keys compare
/// equal under the same rules as `RefKey`'s `Eq` and `Hash` implementations.
pub fn sort_ref_keys(keys: &mut [RefKey]) {
    use std::hash::{Hash, Hasher};
    let mut order: Vec<(u8, u64, u32)> = keys
        .iter()
        .enumerate()
        .map(|(at, key)| {
            let mut hasher = rustc_hash::FxHasher::default();
            key.hash(&mut hasher);
            (key.tag(), hasher.finish(), at as u32)
        })
        .collect();
    order.sort_unstable_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| ref_key_folded_cmp(&keys[a.2 as usize], &keys[b.2 as usize]))
    });
    // Put the key `order[at]` names at `at`, following each cycle of the
    // permutation once.
    for start in 0..order.len() {
        let mut at = start;
        while order[at].2 as usize != start {
            let from = order[at].2 as usize;
            keys.swap(at, from);
            order[at].2 = at as u32;
            at = from;
        }
        order[at].2 = at as u32;
    }
}

pub fn ref_keys_are_sorted(keys: &[RefKey]) -> bool {
    keys.windows(2).all(|pair| {
        pair[0].tag() < pair[1].tag()
            || (pair[0].tag() == pair[1].tag()
                && ref_key_order_cmp(&pair[0], &pair[1]) != std::cmp::Ordering::Greater)
    })
}

fn ref_key_order_cmp(a: &RefKey, b: &RefKey) -> std::cmp::Ordering {
    use std::hash::{Hash, Hasher};
    let mut ah = rustc_hash::FxHasher::default();
    let mut bh = rustc_hash::FxHasher::default();
    a.hash(&mut ah);
    b.hash(&mut bh);
    ah.finish()
        .cmp(&bh.finish())
        .then_with(|| ref_key_folded_cmp(a, b))
}

fn ref_key_folded_cmp(a: &RefKey, b: &RefKey) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (RefKey::Class(a), RefKey::Class(b)) | (RefKey::Function(a), RefKey::Function(b)) => {
            a.lower().as_bytes().cmp(b.lower().as_bytes())
        }
        (RefKey::Constant(a), RefKey::Constant(b)) => {
            let (a_namespace, a_name) = constant_parts(a);
            let (b_namespace, b_name) = constant_parts(b);
            cmp_ascii_folded(a_namespace, b_namespace).then_with(|| a_name.cmp(b_name))
        }
        (RefKey::GlobalVar(a), RefKey::GlobalVar(b)) => a.as_bytes().cmp(b.as_bytes()),
        _ => Ordering::Equal,
    }
}

fn cmp_ascii_folded(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// `name` without a leading `\`.
fn without_backslash(name: &Name) -> Name {
    match name.strip_prefix('\\') {
        Some(rest) => Name::from(rest.trim_start_matches('\\')),
        None => name.clone(),
    }
}

/// A constant name's namespace and its own name, which the first is
/// compared ignoring case and the second as written.
fn constant_parts(name: &str) -> (&str, &str) {
    match name.rfind('\\') {
        Some(at) => (&name[..at], &name[at + 1..]),
        None => ("", name),
    }
}

impl PartialEq for RefKey {
    fn eq(&self, other: &RefKey) -> bool {
        match (self, other) {
            (RefKey::Class(a), RefKey::Class(b)) | (RefKey::Function(a), RefKey::Function(b)) => {
                a.eq_ignore_case(b)
            }
            (RefKey::Constant(a), RefKey::Constant(b)) => {
                let ((a_namespace, a_name), (b_namespace, b_name)) =
                    (constant_parts(a), constant_parts(b));
                a_name == b_name && a_namespace.eq_ignore_ascii_case(b_namespace)
            }
            (RefKey::GlobalVar(a), RefKey::GlobalVar(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for RefKey {}

impl std::hash::Hash for RefKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u8(self.tag());
        match self {
            RefKey::Class(name) | RefKey::Function(name) => state.write_u64(name.folded_hash()),
            RefKey::Constant(name) => {
                let (namespace, own) = constant_parts(name);
                state.write_u64(crate::name::folded_hash(namespace.as_bytes()));
                state.write_u64(crate::name::text_hash(own));
            }
            RefKey::GlobalVar(name) => state.write_u64(name.text_hash()),
        }
    }
}

/// A doc comment the parser attached to a node, with where it is and the
/// names in effect there.
#[derive(Debug, Clone)]
pub struct DocSite {
    pub doc: Arc<str>,
    /// Where the node starts.
    pub pos: u32,
    /// Whether the node is outside every function and method body.
    pub outside: bool,
    /// The namespace and imports at the node.
    pub scope: Arc<NameContext>,
}

/// What a function or a class with a doc comment encloses: the doc
/// comments of the nodes within it (its parameters, its body, its members)
/// are read with the templates its own declares in scope. A node is within
/// when it starts after `start`, where the declaration starts, and before
/// `end`.
#[derive(Debug, Clone)]
pub struct DocFrame {
    pub doc: Arc<str>,
    pub start: u32,
    pub end: u32,
}

/// Every name a file refers to, as the keys of what each is, and the doc
/// comments to read the rest from. Used to decide which files might need
/// re-checking when a declaration changes: missing a real reference would
/// let a stale diagnostic survive an edit elsewhere, while an extra name
/// costs one harmless re-check, so this errs toward over-collecting. An
/// unqualified function or constant name in a namespace yields both the
/// namespaced and the global name.
#[derive(Debug, Clone, Default)]
pub struct FileReferences {
    /// Every name outside the doc comments.
    pub names: FxHashSet<RefKey>,
    /// Those outside every function and method body: what the file's
    /// declarations can take their shape from (signatures, hierarchies,
    /// constant values).
    pub outside: FxHashSet<RefKey>,
    pub doc_sites: Vec<DocSite>,
    pub doc_frames: Vec<DocFrame>,
}

/// Records what the parser meets.
#[derive(Default)]
pub(crate) struct Recorder {
    references: FileReferences,
    // How many function and method bodies the parse is inside.
    body_depth: u32,
    // The scope last handed to a doc site, which a later one shares while the
    // names in effect are the same.
    published: Option<Arc<NameContext>>,
}

impl Recorder {
    pub(crate) fn into_references(self) -> FileReferences {
        self.references
    }

    /// A recorder for a part of the file parsed on its own (a string's
    /// `{$...}`), inside as many bodies as this one.
    pub(crate) fn fork(&self) -> Recorder {
        Recorder {
            references: FileReferences::default(),
            body_depth: self.body_depth,
            published: None,
        }
    }

    /// What a part of the file referred to, as `fork` recorded it.
    pub(crate) fn absorb(&mut self, part: FileReferences) {
        self.references.names.extend(part.names);
        self.references.outside.extend(part.outside);
        self.references.doc_sites.extend(part.doc_sites);
        self.references.doc_frames.extend(part.doc_frames);
    }

    /// Records `key` as referenced.
    pub(crate) fn add(&mut self, key: RefKey) {
        if self.body_depth == 0 {
            self.references.outside.insert(key.clone());
        }
        self.references.names.insert(key);
    }

    pub(crate) fn enter_body(&mut self) {
        self.body_depth += 1;
    }

    pub(crate) fn leave_body(&mut self) {
        self.body_depth -= 1;
    }

    /// Records a resolved name as a reference to what `kind` says it is:
    /// the one name it resolved to, and for an unqualified function or
    /// constant in a namespace the global one it falls back to; a name that
    /// is not resolved is taken as written, unless it is a keyword.
    pub(crate) fn name(&mut self, name: &QualifiedName, kind: NameKind) {
        let key = |name: &Name| match kind {
            NameKind::Class => RefKey::class(name),
            NameKind::Function => RefKey::function(name),
            NameKind::Constant => RefKey::constant(name),
        };
        match &name.resolved {
            ResolvedName::Exact(resolved) => self.add(key(resolved)),
            ResolvedName::WithGlobalFallback(resolved) => {
                self.add(key(resolved));
                self.add(key(&name.written));
            }
            ResolvedName::Unresolved => {
                if !["self", "parent", "static"]
                    .iter()
                    .any(|keyword| name.is_keyword(keyword))
                {
                    self.add(key(&name.written));
                }
            }
        }
    }

    /// An import's path.
    pub(crate) fn import(&mut self, kind: UseKind, path: &Name) {
        self.add(match kind {
            UseKind::Class => RefKey::class(path),
            UseKind::Function => RefKey::function(path),
            UseKind::Const => RefKey::constant(path),
        });
    }

    /// A variable a `global` statement names.
    pub(crate) fn global_variable(&mut self, name: &str) {
        self.add(RefKey::global_variable(name));
    }

    /// A doc comment on the node that starts at `pos`, with `ctx` in effect.
    pub(crate) fn doc(&mut self, doc: &Option<Arc<str>>, pos: u32, ctx: &NameContext) {
        let Some(doc) = doc else { return };
        let scope = match &self.published {
            Some(scope) if scope.version() == ctx.version() => Arc::clone(scope),
            _ => {
                let scope = Arc::new(ctx.clone());
                self.published = Some(Arc::clone(&scope));
                scope
            }
        };
        self.references.doc_sites.push(DocSite {
            doc: Arc::clone(doc),
            pos,
            outside: self.body_depth == 0,
            scope,
        });
    }

    /// What a declaration whose doc comment is `doc` encloses.
    pub(crate) fn frame(&mut self, doc: &Option<Arc<str>>, start: u32, end: u32) {
        if let Some(doc) = doc {
            self.references.doc_frames.push(DocFrame {
                doc: Arc::clone(doc),
                start,
                end,
            });
        }
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    fn by_comparator(keys: &mut [RefKey]) {
        use std::hash::{Hash, Hasher};
        keys.sort_unstable_by(|a, b| {
            let mut ah = rustc_hash::FxHasher::default();
            let mut bh = rustc_hash::FxHasher::default();
            a.hash(&mut ah);
            b.hash(&mut bh);
            a.tag()
                .cmp(&b.tag())
                .then_with(|| ah.finish().cmp(&bh.finish()))
                .then_with(|| ref_key_folded_cmp(a, b))
        });
    }

    #[test]
    fn the_cached_hash_sort_gives_the_order_of_the_comparator_that_hashed_each_time() {
        let mut keys = Vec::new();
        for i in 0..500 {
            keys.push(RefKey::class_text(&format!("App\\Mod{}\\Class{i}", i % 7)));
            keys.push(RefKey::function_text(&format!("app\\f{i}")));
            keys.push(RefKey::constant_text(&format!("App\\Mod{}\\C{i}", i % 3)));
            keys.push(RefKey::global_variable(&format!("g{i}")));
        }
        let mut expected = keys.clone();
        by_comparator(&mut expected);
        sort_ref_keys(&mut keys);
        assert_eq!(keys.len(), expected.len());
        assert!(
            keys.iter()
                .zip(&expected)
                .all(|(a, b)| a.tag() == b.tag() && a.name().as_str() == b.name().as_str())
        );
        assert!(ref_keys_are_sorted(&keys));
    }
}
