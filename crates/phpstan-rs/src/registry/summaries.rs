//! What the finishing passes read of a function or method body, in symbolic
//! form: made once, for every body of a file, by one walk over the body
//! (on whichever thread parses the file), and resolved against the
//! registry by each pass. A pass reads a summary and not the statements, so
//! nothing but the summary has to be kept of a body to settle its verdicts.
//!
//! - Purity (`registry::purity`): whether a construct that makes a body
//!   impure occurs in it, and the calls whose own purity decides the rest.
//! - Effects (`registry::effects`): the places the body writes, what its
//!   variables are bound to, and the calls it inherits writes from.
//! - Assigned properties (`checker::uninitialized`): the properties the body
//!   assigns anywhere, on `$this` or on a variable it made an instance of.

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::{Visitor, walk_class_def, walk_expr, walk_function_decl, walk_stmt};

/// A call a body's purity is conditional on, in the form the body wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PurityCall {
    /// A function called by name.
    Function(QualifiedName),
    /// A method called on `$this`.
    ThisMethod(Name),
    /// A static call on a class named by `self`, `static`, `parent` or a
    /// name, of a method.
    Static(QualifiedName, Name),
    /// `new` of a class named by `self`, `static` or a name.
    New(QualifiedName),
}

/// Where a write lands, as the body wrote it: a root, and the properties
/// and elements walked from it, the root's first.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Place {
    pub(crate) root: Root,
    pub(crate) steps: Box<[Step]>,
}

/// What a place starts from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Root {
    /// `$this`.
    This,
    /// A variable other than `$this` and the superglobals; `None` for one
    /// whose name is computed (`$$n`, `${…}`) or not written (`extract`).
    Variable(Option<Name>),
    /// `X::$p`, the class as written; `None` for a computed property name.
    Static(QualifiedName, Option<Name>),
    /// A static property of a class computed at run time (`$c::$p`).
    DynamicStatic(Option<Name>),
    /// A global: `$GLOBALS['x']` or a superglobal, by name; `None` for
    /// `$GLOBALS[$k]`.
    Global(Option<Name>),
    /// Any other value: a call's result, a new object, a literal.
    Value,
}

/// One step from a place's root.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Step {
    /// `->p`; `None` for a computed name.
    Property(Option<Name>),
    /// `[k]`.
    Element,
}

impl Place {
    fn root(root: Root) -> Place {
        Place {
            root,
            steps: Box::new([]),
        }
    }

    /// Any other value: a call's result, a new object, a literal.
    pub(crate) fn value() -> Place {
        Place::root(Root::Value)
    }

    /// This place, then `step`.
    pub(crate) fn then(self, step: Step) -> Place {
        let mut steps = self.steps.into_vec();
        steps.push(step);
        Place {
            root: self.root,
            steps: steps.into_boxed_slice(),
        }
    }

    /// The place `e` names, when it names one: a variable, a property, an
    /// element, a static property or a global, through any nesting.
    pub(crate) fn of(e: &Expr) -> Option<Place> {
        syntax::stack::guard(|| {
            Some(match &e.peeled().kind {
                ExprKind::Variable(v) if **v == *"this" => Place::root(Root::This),
                ExprKind::Variable(v) if is_superglobal(v) && **v != *"GLOBALS" => {
                    Place::root(Root::Global(Some(v.clone())))
                }
                ExprKind::Variable(v) => Place::root(Root::Variable(Some(v.clone()))),
                ExprKind::DynamicVariable(_) => Place::root(Root::Variable(None)),
                ExprKind::PropertyAccess(base, prop, _) => Place::of(base)
                    .unwrap_or_else(|| Place::root(Root::Value))
                    .then(Step::Property(Some(prop.name.clone()))),
                ExprKind::DynamicPropertyAccess(base, _, _) => Place::of(base)
                    .unwrap_or_else(|| Place::root(Root::Value))
                    .then(Step::Property(None)),
                ExprKind::Index(base, key) => {
                    if matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"GLOBALS") {
                        let name = key
                            .as_deref()
                            .and_then(literal_string_value)
                            .map(|name| Name::from(&*name));
                        return Some(Place::root(Root::Global(name)));
                    }
                    Place::of(base)
                        .unwrap_or_else(|| Place::root(Root::Value))
                        .then(Step::Element)
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Name(class), prop) => {
                    Place::root(Root::Static((**class).clone(), Some(prop.name.clone())))
                }
                ExprKind::StaticPropertyFetch(StaticTarget::Expr(_), prop) => {
                    Place::root(Root::DynamicStatic(Some(prop.name.clone())))
                }
                ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class), _) => {
                    Place::root(Root::Static((**class).clone(), None))
                }
                ExprKind::DynamicStaticPropertyFetch(StaticTarget::Expr(_), _) => {
                    Place::root(Root::DynamicStatic(None))
                }
                _ => return None,
            })
        })
    }

    /// The variable this place is, with no step from it.
    fn variable(&self) -> Option<&Name> {
        match (&self.root, &*self.steps) {
            (Root::Variable(Some(name)), []) => Some(name),
            _ => None,
        }
    }
}

/// What a variable of a body is bound to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Binding {
    /// `global $x`: the global of that name; `None` for `global $$n`.
    Global(Option<Name>),
    /// `static $x`: the declaration's static variable.
    Static,
    /// `$x = &place`, `foreach (place as &$x)`: a reference to the place.
    Reference(Place),
}

/// What a variable only ever holds, when it holds nothing the body did not
/// make.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Made {
    /// An object the body made: by `new` of the class named (`self`,
    /// `static` and `parent` as written), or of a class not named (`clone`,
    /// `new $c`, `new class`).
    Object(Option<QualifiedName>),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote: one of those its summary lists at these indices.
    Closure(Box<[u32]>),
}

impl Made {
    /// What a variable holds that holds what `self` or `other` says: `None`
    /// when that is not only what the body made.
    pub(crate) fn join(&self, other: &Made) -> Option<Made> {
        match (self, other) {
            (Made::Closure(a), Made::Closure(b)) => {
                let mut ids: Vec<u32> = a.iter().chain(b.iter()).copied().collect();
                ids.sort_unstable();
                ids.dedup();
                Some(Made::Closure(ids.into_boxed_slice()))
            }
            (Made::Object(a), Made::Object(b)) => Some(Made::Object(
                a.as_ref()
                    .zip(b.as_ref())
                    .filter(|(a, b)| a.written.eq_ignore_ascii_case(&b.written))
                    .map(|(a, _)| a.clone()),
            )),
            _ => None,
        }
    }
}

/// The object a method is called on, as far as the body shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Receiver {
    /// `$this`.
    This,
    /// A property of `$this`.
    ThisProperty(Name),
    /// Another variable.
    Variable(Name),
    /// A static property, the class as written.
    Static(QualifiedName, Name),
    /// Any other value: a call's result, an element, a literal.
    Unknown,
}

/// What a call is to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Callee {
    Function(QualifiedName),
    /// `$receiver->method()`; `None` for a computed method name.
    Method {
        receiver: Receiver,
        method: Option<Name>,
    },
    /// `Class::method()`, the class as written; `None` for a class computed
    /// at run time, or a computed method name.
    Static {
        class: Option<QualifiedName>,
        method: Option<Name>,
    },
    /// `new Class`; `None` for a class computed at run time.
    New(Option<QualifiedName>),
    /// `new class(...)`, whose class has this name.
    Anonymous(Option<Name>),
    /// A value called: the variable holding it, or `None` for any other
    /// expression.
    Value(Option<Name>),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote, called where it is written: one of those its summary lists at
    /// these indices.
    Written(Box<[u32]>),
}

/// Where an argument is passed.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ArgAt {
    Position(u32),
    Named(Name),
    /// `...$args`, and every argument after one.
    Spread,
}

/// A callable an argument names, as the body wrote it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Callable {
    /// A string: a function's name, or `Class::method`.
    Text(Name),
    /// `[target, 'method']`.
    Pair(PairTarget, Name),
    /// A closure, an arrow function or a first-class callable the body
    /// wrote, by the index its summary lists it at.
    Written(u32),
}

/// The first element of an array callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum PairTarget {
    /// An object: `$this`, a property of it, a variable or a static
    /// property, as a method's receiver.
    Object(Receiver),
    /// A class, by a string or `Class::class`.
    Class(QualifiedName),
}

impl Summarizer {
    /// The callable `e` names, when the body wrote one.
    fn callable(&mut self, e: &Expr) -> Option<Callable> {
        let e = e.peeled();
        match &e.kind {
            ExprKind::Literal(Literal::String(text)) => {
                Some(Callable::Text(Name::from(text.as_str()?)))
            }
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                Some(Callable::Written(self.closure_id(e)))
            }
            ExprKind::ArrayLiteral(items) => {
                let [first, second] = &items[..] else {
                    return None;
                };
                let positional = |item: &ArrayItem, at: i64| {
                    !item.spread
                        && !item.by_ref
                        && match &item.key {
                            None => true,
                            Some(key) => {
                                matches!(&key.peeled().kind, ExprKind::Literal(Literal::Int(n)) if *n == at)
                            }
                        }
                };
                if !positional(first, 0) || !positional(second, 1) {
                    return None;
                }
                let method = literal_string_value(&second.value)?;
                let target = match &first.value.peeled().kind {
                    // A class a string names is fully qualified.
                    ExprKind::Literal(Literal::String(text)) => {
                        let name = Name::from(text.as_str()?.trim_start_matches('\\'));
                        PairTarget::Class(QualifiedName {
                            written: name.clone(),
                            resolved: ResolvedName::Exact(name),
                            span: first.value.span,
                        })
                    }
                    ExprKind::ClassNameFetch(StaticTarget::Name(class)) => {
                        PairTarget::Class((**class).clone())
                    }
                    _ => PairTarget::Object(receiver(&first.value)),
                };
                Some(Callable::Pair(target, Name::from(&*method)))
            }
            ExprKind::Assign(_, rhs) => syntax::stack::guard(|| self.callable(rhs)),
            _ => None,
        }
    }
}

/// A call in a body, with its arguments that are places, where a
/// by-reference parameter writes them, and those that name a callable.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CallWrites {
    pub(crate) callee: Callee,
    /// Where each argument is passed.
    pub(crate) arguments: Box<[ArgAt]>,
    pub(crate) places: Box<[(ArgAt, Place)]>,
    pub(crate) callables: Box<[(ArgAt, Callable)]>,
}

/// What a body or a closure in it writes, before any declaration is looked
/// up. A closure, an arrow function or a first-class callable it writes is
/// not run by being written: what it writes is its own.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WriteFacts {
    /// The variables the body assigns, which a declared type of a parameter
    /// then no longer describes.
    pub(crate) reassigned: Box<[Name]>,
    /// Every place the body writes.
    pub(crate) writes: Box<[Place]>,
    /// What its variables are bound to; `None` for a variable whose name is
    /// computed (`global $$n`), which may be any.
    pub(crate) bindings: Box<[(Option<Name>, Binding)]>,
    /// The variables written, each with what it only ever holds when that is
    /// what the body made, `None` otherwise. A body's own lists only those
    /// that hold what it made; a closure's every variable it writes, since
    /// one it takes from the code that wrote it holds what that code made
    /// only while the closure writes it nothing else.
    pub(crate) made: Box<[(Name, Option<Made>)]>,
    /// A construct that may write anything: `eval` or `include`.
    pub(crate) anything: bool,
    /// Every call, in the order the body makes them.
    pub(crate) calls: Box<[CallWrites]>,
}

/// A closure, an arrow function or a first-class callable a body wrote.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClosureFacts {
    /// The closure it is written in, by its index; `None` when the body
    /// wrote it itself.
    pub(crate) within: Option<u32>,
    /// Its parameters, in order.
    pub(crate) params: Box<[ClosureParam]>,
    /// The variables it takes from the code that wrote it, each with whether
    /// it takes it by reference; `None` for every variable but its
    /// parameters, by value, as an arrow function and a first-class callable
    /// take them.
    pub(crate) captures: Option<Box<[(Name, bool)]>>,
    pub(crate) facts: WriteFacts,
}

/// A closure's or an arrow function's parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClosureParam {
    pub(crate) name: Name,
    /// The classes its declared type admits objects of (`self`, `static` and
    /// `parent` as written); `None` for an object of any class.
    pub(crate) classes: Option<Box<[QualifiedName]>>,
    /// Taken by reference, or variadic: its value is not its declared type's
    /// all through the body.
    pub(crate) untyped: bool,
}

impl ClosureParam {
    fn of(param: &Param) -> ClosureParam {
        ClosureParam {
            name: param.name.name.clone(),
            classes: hint_classes(param.type_hint.as_ref()),
            untyped: param.by_ref || param.variadic,
        }
    }
}

/// The classes a value of the declared type `hint` may be an object of;
/// `None` for any class.
fn hint_classes(hint: Option<&TypeHint>) -> Option<Box<[QualifiedName]>> {
    fn add(hint: &TypeHint, out: &mut Vec<QualifiedName>) -> Option<()> {
        let keyword = |name: &str| QualifiedName {
            written: Name::from(name),
            resolved: ResolvedName::Exact(Name::from(name)),
            span: hint.span,
        };
        match &hint.kind {
            TypeHintKind::Keyword(native, _) => match native {
                NativeType::Object | NativeType::Mixed | NativeType::Callable => return None,
                NativeType::Iterable => out.push(keyword("Traversable")),
                NativeType::SelfType => out.push(keyword("self")),
                NativeType::Static => out.push(keyword("static")),
                NativeType::Parent => out.push(keyword("parent")),
                NativeType::Int
                | NativeType::Float
                | NativeType::String
                | NativeType::Bool
                | NativeType::Array
                | NativeType::Void
                | NativeType::Never
                | NativeType::Null
                | NativeType::True
                | NativeType::False => {}
            },
            TypeHintKind::Name(name) => out.push(name.clone()),
            TypeHintKind::Nullable(inner) => add(inner, out)?,
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in members {
                    add(member, out)?;
                }
            }
        }
        Some(())
    }
    let mut out = Vec::new();
    add(hint?, &mut out)?;
    Some(out.into_boxed_slice())
}

/// A body, read by the passes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodySummary {
    /// The calls the body's purity is conditional on, or `None` when the body
    /// holds a construct that makes it impure whatever they are.
    pub(crate) purity: Option<Box<[PurityCall]>>,
    pub(crate) writes: WriteFacts,
    /// The closures, arrow functions and first-class callables the body
    /// wrote, those written in them too, each by its index; one is listed
    /// after the one it is written in.
    pub(crate) closures: Box<[ClosureFacts]>,
    /// Whether the body has no statements.
    pub(crate) empty: bool,
    /// The properties the body assigns, when it assigns any.
    pub(crate) assigned: Option<Box<AssignedProperties>>,
}

impl BodySummary {
    /// The summary of code that only writes `e`, a closure, an arrow
    /// function or a first-class callable: the first closure it lists.
    pub(crate) fn of_closure(e: &Expr) -> BodySummary {
        let mut walk = Summarizer::default();
        walk.visit_expr(e.peeled());
        walk.finish()
    }

    /// The summary of the statements of one body.
    pub fn of(body: &[Stmt]) -> BodySummary {
        let mut walk = Summarizer::default();
        syntax::visit::walk_stmts(&mut walk, body);
        let mut summary = walk.finish();
        summary.empty = body.is_empty();
        summary
    }
}

/// The properties a body assigns anywhere in its statements, however deeply
/// nested and however conditional, outside closures: `$this->p = ...`,
/// `$this->p ??= ...`, and the same on a local variable the body
/// assigned a `new` expression.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AssignedProperties {
    /// The properties assigned on `$this`.
    on_this: Box<[Name]>,
    /// The variable and the property assigned on it, for each assignment.
    on_variables: Box<[(Name, Name)]>,
    /// The variable and the class named by the `new` it was assigned (a
    /// leading backslash trimmed; `self` and `static` as written).
    new_instances: Box<[(Name, Name)]>,
}

/// Collects what the expression statements of a body assign.
#[derive(Default)]
struct AssignedBuilder {
    on_this: Vec<Name>,
    on_variables: Vec<(Name, Name)>,
    new_instances: Vec<(Name, Name)>,
}

impl AssignedBuilder {
    /// The expression of a statement of the body, outside its closures.
    fn statement(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Assign(lhs, rhs) => {
                if let ExprKind::Variable(v) = &lhs.kind
                    && let Some(class) = new_instance_class(rhs)
                {
                    self.new_instances.push((v.clone(), class));
                }
                record_property_assigned(lhs, &mut self.on_this, &mut self.on_variables);
            }
            ExprKind::AssignRef(lhs, _) | ExprKind::CompoundAssign(BinOp::Coalesce, lhs, _) => {
                record_property_assigned(lhs, &mut self.on_this, &mut self.on_variables);
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Option<Box<AssignedProperties>> {
        if self.on_this.is_empty() && self.on_variables.is_empty() {
            return None;
        }
        self.on_this.sort();
        self.on_this.dedup();
        self.on_variables.sort();
        self.on_variables.dedup();
        self.new_instances.sort();
        self.new_instances.dedup();
        Some(Box::new(AssignedProperties {
            on_this: self.on_this.into_boxed_slice(),
            on_variables: self.on_variables.into_boxed_slice(),
            new_instances: self.new_instances.into_boxed_slice(),
        }))
    }
}

impl AssignedProperties {
    /// Whether the body assigns `prop` on `$this` or on a variable it made
    /// an instance of `class_name` (or of `self` or `static`).
    pub(crate) fn assigns(&self, prop: &str, class_name: &str) -> bool {
        if self.on_this.iter().any(|name| **name == *prop) {
            return true;
        }
        self.on_variables.iter().any(|(variable, assigned)| {
            **assigned == *prop
                && self
                    .new_instances
                    .iter()
                    .any(|(v, class)| v == variable && is_self_instance_class(class, class_name))
        })
    }
}

/// The class a `new` names, at the end of a chain of assignments
/// (`$a = $b->p = new C`).
fn new_instance_class(e: &Expr) -> Option<Name> {
    syntax::stack::guard(|| match &e.kind {
        ExprKind::New(callee, _) => {
            let ExprKind::Name(name) = &callee.kind else {
                return None;
            };
            Some(Name::from(name.trim_start_matches('\\')))
        }
        ExprKind::Assign(_, rhs) => new_instance_class(rhs),
        _ => None,
    })
}

fn is_self_instance_class(class: &str, class_name: &str) -> bool {
    class.eq_ignore_ascii_case("self")
        || class.eq_ignore_ascii_case("static")
        || class == class_name
        || class.ends_with(&format!("\\{class_name}"))
}

fn record_property_assigned(
    lhs: &Expr,
    on_this: &mut Vec<Name>,
    on_variables: &mut Vec<(Name, Name)>,
) {
    let ExprKind::PropertyAccess(base, property, _) = &lhs.kind else {
        return;
    };
    let ExprKind::Variable(v) = &base.kind else {
        return;
    };
    if **v == *"this" {
        on_this.push(property.name.clone());
    } else {
        on_variables.push((v.clone(), property.name.clone()));
    }
}

/// What one scope writes, as the walk finds it: the body's own, or a
/// closure's.
#[derive(Default)]
struct Scope {
    reassigned: FxHashSet<Name>,
    writes: Vec<Place>,
    bindings: Vec<(Option<Name>, Binding)>,
    // What each variable written holds: what the body made, or `None` once
    // it is written anything else.
    made: FxHashMap<Name, Option<Made>>,
    anything: bool,
    calls: Vec<CallWrites>,
}

impl Scope {
    /// The scope's facts; `all_written` keeps every variable written in
    /// `made`, not only those holding what the body made.
    fn finish(self, all_written: bool) -> WriteFacts {
        let mut reassigned: Vec<Name> = self.reassigned.into_iter().collect();
        reassigned.sort();
        let mut made: Vec<(Name, Option<Made>)> = self
            .made
            .into_iter()
            .filter(|(_, made)| all_written || made.is_some())
            .collect();
        made.sort_by(|a, b| a.0.cmp(&b.0));
        WriteFacts {
            reassigned: reassigned.into_boxed_slice(),
            writes: self.writes.into_boxed_slice(),
            bindings: self.bindings.into_boxed_slice(),
            made: made.into_boxed_slice(),
            anything: self.anything,
            calls: self.calls.into_boxed_slice(),
        }
    }
}

/// A scope the walk is inside of, while it walks a closure written in it.
struct Enclosing {
    scope: Scope,
    // The variables the closure written in it takes from it by reference.
    shared: FxHashSet<Name>,
}

#[derive(Default)]
struct Summarizer {
    // How many closures and arrow functions the walk is inside: they are
    // not run by being made, so their statements are no part of the body's
    // purity.
    closures: u32,
    impure: bool,
    purity_calls: Vec<PurityCall>,
    assigned: AssignedBuilder,
    scope: Scope,
    // The scopes around the one the walk is in, the outermost first.
    enclosing: Vec<Enclosing>,
    // The closure the walk is in, by its index; `None` in the body's own
    // scope.
    current: Option<u32>,
    // Each closure's index, by the address of its expression.
    ids: FxHashMap<usize, u32>,
    written: Vec<ClosureFacts>,
}

impl Summarizer {
    fn finish(self) -> BodySummary {
        BodySummary {
            purity: (!self.impure).then(|| self.purity_calls.into_boxed_slice()),
            writes: self.scope.finish(false),
            closures: self.written.into_boxed_slice(),
            empty: false,
            assigned: self.assigned.finish(),
        }
    }

    /// The index of the closure, arrow function or first-class callable
    /// `e`, given it when it is first met.
    fn closure_id(&mut self, e: &Expr) -> u32 {
        let next = self.written.len() as u32;
        let id = *self
            .ids
            .entry(std::ptr::from_ref(e) as usize)
            .or_insert(next);
        if id == next {
            // Until the walk reaches it, it may write anything.
            self.written.push(ClosureFacts {
                within: None,
                params: Box::new([]),
                captures: None,
                facts: WriteFacts {
                    anything: true,
                    ..WriteFacts::default()
                },
            });
        }
        id
    }

    /// Walks the closure, arrow function or first-class callable `e` in a
    /// scope of its own, which takes `captures` from the one the walk is in.
    fn closure(
        &mut self,
        e: &Expr,
        params: &[Param],
        captures: Option<Box<[(Name, bool)]>>,
        walk: impl FnOnce(&mut Summarizer),
    ) {
        let id = self.closure_id(e);
        let shared = captures
            .iter()
            .flatten()
            .filter(|(_, by_ref)| *by_ref)
            .map(|(name, _)| name.clone())
            .collect();
        self.enclosing.push(Enclosing {
            scope: std::mem::take(&mut self.scope),
            shared,
        });
        let within = self.current.replace(id);
        walk(self);
        let Some(enclosing) = self.enclosing.pop() else {
            unreachable!("pushed above");
        };
        let scope = std::mem::replace(&mut self.scope, enclosing.scope);
        self.current = within;
        self.written[id as usize] = ClosureFacts {
            within,
            params: params.iter().map(ClosureParam::of).collect(),
            captures,
            facts: scope.finish(true),
        };
    }

    /// The scope the walk is in, and each around it sharing the variable
    /// `name` with it by reference.
    fn scopes_sharing(&mut self, name: &Name) -> Vec<&mut Scope> {
        let mut out = vec![&mut self.scope];
        for enclosing in self.enclosing.iter_mut().rev() {
            if !enclosing.shared.contains(name) {
                break;
            }
            out.push(&mut enclosing.scope);
        }
        out
    }

    /// A construct that makes the body impure, outside a closure.
    fn impure_unless_in_closure(&mut self, impure: bool) {
        if impure && self.closures == 0 {
            self.impure = true;
        }
    }

    fn purity_call(&mut self, call: PurityCall) {
        if self.closures == 0 && !self.impure {
            self.purity_calls.push(call);
        }
    }

    /// An assignment to `target`: each variable it names is reassigned.
    fn reassign(&mut self, target: &Expr) {
        let mut names = FxHashSet::default();
        collect_assigned(target, &mut names);
        for name in names {
            for scope in self.scopes_sharing(&name) {
                scope.reassigned.insert(name.clone());
            }
        }
    }

    /// A write to `target`: to each target of a destructuring, to the place
    /// `target` names otherwise. A variable written holds what the body
    /// made when `made` says what.
    fn write(&mut self, target: &Expr, made: Option<Made>) {
        if let ExprKind::Destructure(items, _) = &target.peeled().kind {
            for item in items {
                if let Some(target) = &item.target {
                    self.write(target, None);
                }
            }
            return;
        }
        let Some(place) = Place::of(target) else {
            return;
        };
        if let Some(name) = place.variable() {
            self.note_variable(name.clone(), made);
        }
        self.scope.writes.push(place);
    }

    /// That the variable `name` is written what `made` says, or anything
    /// else when `made` is `None`: in the scope the walk is in, and in those
    /// sharing it by reference.
    fn note_variable(&mut self, name: Name, made: Option<Made>) {
        for scope in self.scopes_sharing(&name) {
            let held = scope
                .made
                .entry(name.clone())
                .or_insert_with(|| made.clone());
            *held = match (held.take(), &made) {
                (Some(a), Some(b)) => a.join(b),
                _ => None,
            };
        }
    }

    fn bind(&mut self, variable: Option<Name>, binding: Binding) {
        if let Some(name) = &variable {
            self.scope.made.insert(name.clone(), None);
        }
        self.scope.bindings.push((variable, binding));
    }

    /// A call, with the arguments that are places and those that name a
    /// callable.
    fn call(&mut self, callee: Callee, args: &[Arg]) {
        let mut spread = false;
        let mut arguments = Vec::new();
        let mut places = Vec::new();
        let mut callables = Vec::new();
        for (at, arg) in args.iter().enumerate() {
            spread |= arg.spread;
            let at = match &arg.name {
                _ if spread => ArgAt::Spread,
                Some(name) => ArgAt::Named(name.name.clone()),
                None => ArgAt::Position(at as u32),
            };
            arguments.push(at.clone());
            if let Some(callable) = self.callable(&arg.value) {
                callables.push((at.clone(), callable));
            }
            if let Some(place) = Place::of(&arg.value) {
                places.push((at, place));
            }
        }
        self.scope.calls.push(CallWrites {
            callee,
            arguments: arguments.into_boxed_slice(),
            places: places.into_boxed_slice(),
            callables: callables.into_boxed_slice(),
        });
    }

    /// What the body made that `e` evaluates to, when it is that.
    fn made_by(&mut self, e: &Expr) -> Option<Made> {
        let e = e.peeled();
        match &e.kind {
            ExprKind::New(callee, _) => Some(Made::Object(match &callee.kind {
                ExprKind::Name(name) => Some(name.clone()),
                _ => None,
            })),
            ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => Some(Made::Object(None)),
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                Some(Made::Closure(Box::new([self.closure_id(e)])))
            }
            ExprKind::Assign(_, rhs) => syntax::stack::guard(|| self.made_by(rhs)),
            _ => None,
        }
    }
}

impl Summarizer {
    /// Records `e` when it is a call.
    fn record_call(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) => {
                    self.purity_call(PurityCall::Function(name.clone()));
                    self.call(Callee::Function(name.clone()), args);
                }
                _ => {
                    self.impure_unless_in_closure(true);
                    let callee = match self.made_by(callee) {
                        Some(Made::Closure(ids)) => Callee::Written(ids),
                        _ => Callee::Value(match &callee.peeled().kind {
                            ExprKind::Variable(name) => Some(name.clone()),
                            _ => None,
                        }),
                    };
                    self.call(callee, args);
                }
            },
            ExprKind::MethodCall(recv, method, args, _) => {
                let on_this = matches!(&recv.kind, ExprKind::Variable(v) if *v == "this");
                if on_this {
                    self.purity_call(PurityCall::ThisMethod(method.name.clone()));
                } else {
                    self.impure_unless_in_closure(true);
                }
                self.call(
                    Callee::Method {
                        receiver: receiver(recv),
                        method: Some(method.name.clone()),
                    },
                    args,
                );
            }
            ExprKind::DynamicMethodCall(recv, _, args, _) => self.call(
                Callee::Method {
                    receiver: receiver(recv),
                    method: None,
                },
                args,
            ),
            ExprKind::StaticCall(target, method, args) => {
                match target.as_name() {
                    Some(name) => {
                        self.purity_call(PurityCall::Static(name.clone(), method.name.clone()));
                    }
                    None => self.impure_unless_in_closure(true),
                }
                self.call(
                    Callee::Static {
                        class: target.as_name().cloned(),
                        method: Some(method.name.clone()),
                    },
                    args,
                );
            }
            ExprKind::DynamicStaticCall(target, _, args) => self.call(
                Callee::Static {
                    class: target.as_name().cloned(),
                    method: None,
                },
                args,
            ),
            ExprKind::New(callee, args) => match &callee.kind {
                ExprKind::Name(name) => {
                    self.purity_call(PurityCall::New(name.clone()));
                    self.call(Callee::New(Some(name.clone())), args);
                }
                _ => {
                    self.impure_unless_in_closure(true);
                    self.call(Callee::New(None), args);
                }
            },
            ExprKind::NewAnonymousClass(class) => {
                self.call(Callee::Anonymous(class.name.clone()), &class.args);
            }
            _ => {}
        }
    }
}

/// The call `e` is, as a body's summary records it.
pub(crate) fn call_of(e: &Expr) -> Option<CallWrites> {
    let mut walk = Summarizer::default();
    walk.record_call(e);
    walk.scope.calls.pop()
}

/// `$obj->prop = ...`/`unset($obj->prop)`: a write to a property of any
/// object, through any depth of array-index nesting.
fn property_write_target(e: &Expr) -> bool {
    syntax::stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::Index(base, _) => property_write_target(base),
        _ => false,
    })
}

/// The object `recv` is, as far as the body shows it.
fn receiver(recv: &Expr) -> Receiver {
    match &recv.peeled().kind {
        ExprKind::Variable(v) if **v == *"this" => Receiver::This,
        ExprKind::Variable(v) => Receiver::Variable(v.clone()),
        ExprKind::PropertyAccess(base, prop, _) if matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"this") => {
            Receiver::ThisProperty(prop.name.clone())
        }
        ExprKind::StaticPropertyFetch(StaticTarget::Name(class), prop) => {
            Receiver::Static((**class).clone(), prop.name.clone())
        }
        _ => Receiver::Unknown,
    }
}

impl<'ast> Visitor<'ast> for Summarizer {
    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        // A function declared in the body is a body of its own, but what it
        // assigns still counts against the parameters' types, as it always
        // did.
        let mut nested = Reassigned(&mut self.scope.reassigned);
        walk_function_decl(&mut nested, function);
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        let mut nested = Reassigned(&mut self.scope.reassigned);
        walk_class_def(&mut nested, class);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        syntax::stack::guard(|| {
            let impure = match &stmt.kind {
                StmtKind::Echo(_)
                | StmtKind::Throw(_)
                | StmtKind::Global(_)
                | StmtKind::StaticVar(_)
                | StmtKind::InlineHtml(_) => true,
                StmtKind::Unset(targets) => targets.iter().any(property_write_target),
                _ => false,
            };
            self.impure_unless_in_closure(impure);
            // A closure's statements are its own.
            if self.closures == 0
                && let StmtKind::Expression(e) = &stmt.kind
            {
                self.assigned.statement(e);
            }
            match &stmt.kind {
                StmtKind::Unset(targets) => {
                    for target in targets {
                        self.write(target, None);
                    }
                }
                StmtKind::Foreach {
                    subject,
                    key,
                    value,
                    by_ref,
                    ..
                } => {
                    if let Some(key) = key {
                        self.write(key, None);
                    }
                    match &value.peeled().kind {
                        ExprKind::Variable(name) if *by_ref => {
                            let element = Place::of(subject)
                                .unwrap_or_else(|| Place::root(Root::Value))
                                .then(Step::Element);
                            self.bind(Some(name.clone()), Binding::Reference(element));
                        }
                        _ => self.write(value, None),
                    }
                    if *by_ref {
                        self.write(subject, None);
                    }
                    for target in key.iter().map(|k| &**k).chain([&**value]) {
                        self.reassign(target);
                    }
                }
                StmtKind::Global(vars) => {
                    for var in vars {
                        match var {
                            GlobalVar::Name(name) => self.bind(
                                Some(name.name.clone()),
                                Binding::Global(Some(name.name.clone())),
                            ),
                            GlobalVar::Dynamic(_) => self.bind(None, Binding::Global(None)),
                        }
                    }
                }
                StmtKind::StaticVar(vars) => {
                    for (name, _) in vars {
                        self.bind(Some(name.name.clone()), Binding::Static);
                    }
                }
                _ => {}
            }
            walk_stmt(self, stmt);
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        syntax::stack::guard(|| {
            let impure = match &e.kind {
                // Any static property access at all, even a read of shared
                // static state.
                ExprKind::ClassConstFetch(..)
                | ExprKind::ClassNameFetch(_)
                | ExprKind::StaticPropertyFetch(..)
                | ExprKind::DynamicStaticPropertyFetch(..) => true,
                ExprKind::Variable(name) => is_superglobal(name),
                ExprKind::Yield(..)
                | ExprKind::YieldFrom(_)
                | ExprKind::ShellExec(_)
                | ExprKind::Print(_)
                | ExprKind::Exit(..)
                | ExprKind::Eval(_)
                | ExprKind::Include(..) => true,
                ExprKind::Assign(lhs, _)
                | ExprKind::AssignRef(lhs, _)
                | ExprKind::CompoundAssign(_, lhs, _) => property_write_target(lhs),
                // A computed method name: any method.
                ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) => true,
                _ => false,
            };
            self.impure_unless_in_closure(impure);
            match &e.kind {
                ExprKind::Assign(lhs, rhs) => {
                    let made = self.made_by(rhs);
                    self.write(lhs, made);
                    self.reassign(lhs);
                }
                ExprKind::CompoundAssign(_, lhs, _) => {
                    self.write(lhs, None);
                    self.reassign(lhs);
                }
                ExprKind::AssignRef(lhs, rhs) => {
                    match &lhs.peeled().kind {
                        ExprKind::Variable(name) if **name != *"this" => {
                            let place = Place::of(rhs).unwrap_or_else(|| Place::root(Root::Value));
                            self.bind(Some(name.clone()), Binding::Reference(place));
                        }
                        _ => self.write(lhs, None),
                    }
                    self.write(rhs, None);
                    self.reassign(lhs);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => self.write(target, None),
                ExprKind::Eval(_) | ExprKind::Include(..) => self.scope.anything = true,
                _ => self.record_call(e),
            }
            match &e.kind {
                ExprKind::Closure(closure) => {
                    let captures = closure
                        .uses
                        .iter()
                        .map(|u| (u.name.name.clone(), u.by_ref))
                        .collect();
                    self.closures += 1;
                    self.closure(e, &closure.params, Some(captures), |walk| {
                        walk_expr(walk, e);
                    });
                    self.closures -= 1;
                }
                ExprKind::ArrowFn(arrow) => {
                    self.closures += 1;
                    self.closure(e, &arrow.params, None, |walk| walk_expr(walk, e));
                    self.closures -= 1;
                }
                // What its call's object or class is evaluates where it is
                // written; the call runs where it is called, with the
                // arguments it is called with.
                ExprKind::FirstClassCallable(call) => {
                    self.impure_unless_in_closure(matches!(
                        call.kind,
                        ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..)
                    ));
                    self.closure(e, &[], None, |walk| {
                        walk.record_call(call);
                        if let Some(own) = walk.scope.calls.last_mut() {
                            own.arguments = Box::new([ArgAt::Spread]);
                        }
                    });
                    walk_expr(self, call);
                }
                _ => walk_expr(self, e),
            }
        })
    }
}

/// Collects the variables a body assigns, in the declarations nested in it
/// too.
struct Reassigned<'a>(&'a mut FxHashSet<Name>);

impl<'ast> Visitor<'ast> for Reassigned<'_> {
    fn visit_expr(&mut self, e: &'ast Expr) {
        if let ExprKind::Assign(lhs, _)
        | ExprKind::AssignRef(lhs, _)
        | ExprKind::CompoundAssign(_, lhs, _) = &e.kind
        {
            collect_assigned(lhs, self.0);
        }
        walk_expr(self, e);
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::Foreach { key, value, .. } = &stmt.kind {
            for target in key.iter().map(|k| &**k).chain([&**value]) {
                collect_assigned(target, self.0);
            }
        }
        walk_stmt(self, stmt);
    }
}

fn collect_assigned(target: &Expr, out: &mut FxHashSet<Name>) {
    match &target.kind {
        ExprKind::Variable(name) => {
            out.insert(name.clone());
        }
        ExprKind::Destructure(items, _) => {
            for item in items {
                if let Some(target) = &item.target {
                    collect_assigned(target, out);
                }
            }
        }
        _ => {}
    }
}

syntax::canonical_enum!(PurityCall {
    Function(name),
    ThisMethod(method),
    Static(class, method),
    New(class),
});

syntax::canonical_struct!(Place { root, steps });

syntax::canonical_enum!(Root {
    This,
    Variable(name),
    Static(class, prop),
    DynamicStatic(prop),
    Global(name),
    Value,
});

syntax::canonical_enum!(Step {
    Property(prop),
    Element,
});

syntax::canonical_enum!(Binding {
    Global(name),
    Static,
    Reference(place),
});

syntax::canonical_enum!(Made {
    Object(class),
    Closure(ids),
});

syntax::canonical_enum!(Receiver {
    This,
    ThisProperty(prop),
    Variable(name),
    Static(class, prop),
    Unknown,
});

syntax::canonical_enum!(Callee {
    Function(name),
    Method { receiver, method },
    Static { class, method },
    New(class),
    Anonymous(name),
    Value(name),
    Written(ids),
});

syntax::canonical_enum!(ArgAt {
    Position(at),
    Named(name),
    Spread,
});

syntax::canonical_enum!(Callable {
    Text(name),
    Pair(target, method),
    Written(id),
});

syntax::canonical_enum!(PairTarget {
    Object(receiver),
    Class(class),
});

syntax::canonical_struct!(CallWrites {
    callee,
    arguments,
    places,
    callables,
});

syntax::canonical_struct!(WriteFacts {
    reassigned,
    writes,
    bindings,
    made,
    anything,
    calls,
});

syntax::canonical_struct!(ClosureFacts {
    within,
    params,
    captures,
    facts,
});

syntax::canonical_struct!(ClosureParam {
    name,
    classes,
    untyped,
});

syntax::canonical_struct!(BodySummary {
    purity,
    writes,
    closures,
    empty,
    assigned,
});

syntax::canonical_struct!(AssignedProperties {
    on_this,
    on_variables,
    new_instances,
});
