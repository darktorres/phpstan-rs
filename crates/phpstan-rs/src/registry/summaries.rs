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

    fn then(self, step: Step) -> Place {
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
    /// A closure or an arrow function the body wrote.
    Closure,
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
}

/// Where an argument is passed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// wrote: what it runs is the body's.
    Written,
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

impl Callable {
    /// The callable `e` names, when the body wrote one.
    fn of(e: &Expr) -> Option<Callable> {
        match &e.peeled().kind {
            ExprKind::Literal(Literal::String(text)) => {
                Some(Callable::Text(Name::from(text.as_str()?)))
            }
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                Some(Callable::Written)
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

/// What a body writes, before any declaration is looked up. A closure's and
/// an arrow function's body count for the body that holds them.
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
    /// The variables that only ever hold what the body made.
    pub(crate) made: Box<[(Name, Made)]>,
    /// A construct that may write anything: `eval` or `include`.
    pub(crate) anything: bool,
    /// Every call, in the order the body makes them.
    pub(crate) calls: Box<[CallWrites]>,
}

/// A body, read by the passes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodySummary {
    /// The calls the body's purity is conditional on, or `None` when the body
    /// holds a construct that makes it impure whatever they are.
    pub(crate) purity: Option<Box<[PurityCall]>>,
    pub(crate) writes: WriteFacts,
    /// Whether the body has no statements.
    pub(crate) empty: bool,
    /// The properties the body assigns, when it assigns any.
    pub(crate) assigned: Option<Box<AssignedProperties>>,
}

impl BodySummary {
    /// The summary of the statements of one body.
    pub fn of(body: &[Stmt]) -> BodySummary {
        let mut walk = Summarizer::default();
        syntax::visit::walk_stmts(&mut walk, body);
        let mut summary = walk.finish();
        summary.empty = body.is_empty();
        // What a walk of the statements alone finds.
        #[cfg(debug_assertions)]
        assert_eq!(
            summary.assigned,
            AssignedProperties::of(body),
            "the walk of a body and the walk of its statements disagree on the properties it assigns"
        );
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
    /// What a walk of the statements of `body` alone finds: the reference
    /// for what the one walk of the summary does.
    #[cfg(debug_assertions)]
    fn of(body: &[Stmt]) -> Option<Box<AssignedProperties>> {
        let mut builder = AssignedBuilder::default();
        for stmt in body {
            syntax::visit::any_stmt_in_body(stmt, |s| {
                if let StmtKind::Expression(e) = &s.kind {
                    builder.statement(e);
                }
                false
            });
        }
        builder.finish()
    }

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

#[derive(Default)]
struct Summarizer {
    // How many closures and arrow functions the walk is inside: they are
    // not run by being made, so their statements are no part of the body's
    // purity. What they write is the body's.
    closures: u32,
    impure: bool,
    purity_calls: Vec<PurityCall>,
    reassigned: FxHashSet<Name>,
    writes: Vec<Place>,
    bindings: Vec<(Option<Name>, Binding)>,
    // What each variable written holds: what the body made, or `None` once
    // it is written anything else.
    made: FxHashMap<Name, Option<Made>>,
    anything: bool,
    calls: Vec<CallWrites>,
    assigned: AssignedBuilder,
}

impl Summarizer {
    fn finish(self) -> BodySummary {
        let mut reassigned: Vec<Name> = self.reassigned.into_iter().collect();
        reassigned.sort();
        let mut made: Vec<(Name, Made)> = self
            .made
            .into_iter()
            .filter_map(|(name, made)| Some((name, made?)))
            .collect();
        made.sort_by(|a, b| a.0.cmp(&b.0));
        BodySummary {
            purity: (!self.impure).then(|| self.purity_calls.into_boxed_slice()),
            writes: WriteFacts {
                reassigned: reassigned.into_boxed_slice(),
                writes: self.writes.into_boxed_slice(),
                bindings: self.bindings.into_boxed_slice(),
                made: made.into_boxed_slice(),
                anything: self.anything,
                calls: self.calls.into_boxed_slice(),
            },
            empty: false,
            assigned: self.assigned.finish(),
        }
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
        self.writes.push(place);
    }

    /// That the variable `name` is written what `made` says, or anything
    /// else when `made` is `None`.
    fn note_variable(&mut self, name: Name, made: Option<Made>) {
        let held = self.made.entry(name).or_insert_with(|| made.clone());
        *held = match (held.take(), made) {
            (Some(Made::Closure), Some(Made::Closure)) => Some(Made::Closure),
            (Some(Made::Object(a)), Some(Made::Object(b))) => Some(Made::Object(
                a.zip(b)
                    .filter(|(a, b)| a.written.eq_ignore_ascii_case(&b.written))
                    .map(|(a, _)| a),
            )),
            _ => None,
        };
    }

    fn bind(&mut self, variable: Option<Name>, binding: Binding) {
        if let Some(name) = &variable {
            self.made.insert(name.clone(), None);
        }
        self.bindings.push((variable, binding));
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
            if let Some(callable) = Callable::of(&arg.value) {
                callables.push((at.clone(), callable));
            }
            if let Some(place) = Place::of(&arg.value) {
                places.push((at, place));
            }
        }
        self.calls.push(CallWrites {
            callee,
            arguments: arguments.into_boxed_slice(),
            places: places.into_boxed_slice(),
            callables: callables.into_boxed_slice(),
        });
    }
}

/// What the body made that `e` evaluates to, when it is that.
fn made_by(e: &Expr) -> Option<Made> {
    syntax::stack::guard(|| match &e.peeled().kind {
        ExprKind::New(callee, _) => Some(Made::Object(match &callee.kind {
            ExprKind::Name(name) => Some(name.clone()),
            _ => None,
        })),
        ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => Some(Made::Object(None)),
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
            Some(Made::Closure)
        }
        ExprKind::Assign(_, rhs) => made_by(rhs),
        _ => None,
    })
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
        let mut nested = Reassigned(&mut self.reassigned);
        walk_function_decl(&mut nested, function);
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        let mut nested = Reassigned(&mut self.reassigned);
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
                        collect_assigned(target, &mut self.reassigned);
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
                    self.write(lhs, made_by(rhs));
                    collect_assigned(lhs, &mut self.reassigned);
                }
                ExprKind::CompoundAssign(_, lhs, _) => {
                    self.write(lhs, None);
                    collect_assigned(lhs, &mut self.reassigned);
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
                    collect_assigned(lhs, &mut self.reassigned);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => self.write(target, None),
                ExprKind::Eval(_) | ExprKind::Include(..) => self.anything = true,
                ExprKind::Call(callee, args) => match &callee.kind {
                    ExprKind::Name(name) => {
                        self.purity_call(PurityCall::Function(name.clone()));
                        self.call(Callee::Function(name.clone()), args);
                    }
                    _ => {
                        self.impure_unless_in_closure(true);
                        let variable = match &callee.peeled().kind {
                            ExprKind::Variable(name) => Some(name.clone()),
                            _ => None,
                        };
                        self.call(Callee::Value(variable), args);
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
            if matches!(e.kind, ExprKind::Closure { .. } | ExprKind::ArrowFn { .. }) {
                self.closures += 1;
                walk_expr(self, e);
                self.closures -= 1;
            } else {
                walk_expr(self, e);
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
