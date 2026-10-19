//! What the finishing passes read of a function or method body, in symbolic
//! form: made once, for every body of a file, by one walk over the body
//! (on whichever thread parses the file), and resolved against the
//! registry by each pass. A pass reads a summary and not the statements, so
//! nothing but the summary has to be kept of a body to settle its verdicts.
//!
//! - Purity (`registry::purity`): whether a construct that makes a body
//!   impure occurs in it, and the calls whose own purity decides the rest.
//! - Property writes (`registry::property_writes`): the writes the body
//!   makes itself and the calls it inherits writes from.
//! - Assigned properties (`checker::uninitialized`): the properties the body
//!   assigns anywhere, on `$this` or on a variable it made an instance of.

use rustc_hash::FxHashSet;
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

/// Whose property an argument is, when the argument is a property of an
/// object (or of several, destructured).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum PropertyOf {
    /// Of the body's own `$this`.
    This,
    /// Of any other object.
    Other,
}

/// The object a method is called on, as far as the body shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Receiver {
    /// `$this`.
    This,
    /// A property of `$this`.
    ThisProperty(Name),
    /// Another variable: a parameter, when the body never assigns it.
    Variable(Name),
}

/// What a call is to.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Callee {
    Function(QualifiedName),
    Method {
        receiver: Receiver,
        method: Name,
    },
    /// `target::method()`, `target` a name.
    Static(QualifiedName, Name),
    New(QualifiedName),
    /// `new class(...)`, whose class has this name.
    Anonymous(Option<Name>),
}

/// A call in a body, with the positions of its arguments that are
/// properties: a by-reference parameter at one of them writes it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CallWrites {
    pub(crate) callee: Callee,
    pub(crate) properties: Box<[(u32, PropertyOf)]>,
}

/// What a body writes, before any declaration is looked up.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct WriteFacts {
    /// The variables the body assigns, which a declared type of a parameter
    /// then no longer describes.
    pub(crate) reassigned: Box<[Name]>,
    /// A write to a property of `$this`.
    pub(crate) writes_own_object: bool,
    /// A write that reaches another object's property, or a construct that
    /// may write anything (`eval`, `include`, a call of no known target).
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
    stack::guard(|| match &e.kind {
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
    writes_own_object: bool,
    anything: bool,
    calls: Vec<CallWrites>,
    assigned: AssignedBuilder,
}

impl Summarizer {
    fn finish(self) -> BodySummary {
        let mut reassigned: Vec<Name> = self.reassigned.into_iter().collect();
        reassigned.sort();
        BodySummary {
            purity: (!self.impure).then(|| self.purity_calls.into_boxed_slice()),
            writes: WriteFacts {
                reassigned: reassigned.into_boxed_slice(),
                writes_own_object: self.writes_own_object,
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

    /// A write to `target`.
    fn write(&mut self, target: &Expr) {
        match property_of(target) {
            Some(PropertyOf::This) => self.writes_own_object = true,
            Some(PropertyOf::Other) => self.anything = true,
            None => {}
        }
    }

    /// A call, with the property targets among `args`.
    fn call(&mut self, callee: Callee, args: &[Arg]) {
        let properties = args
            .iter()
            .enumerate()
            .filter_map(|(at, arg)| Some((at as u32, property_of(&arg.value)?)))
            .collect();
        self.calls.push(CallWrites { callee, properties });
    }
}

/// Whose property a write to `target` writes, when it writes one: an
/// assignment to a list takes the farthest reach of its targets.
fn property_of(target: &Expr) -> Option<PropertyOf> {
    match &target.peeled().kind {
        ExprKind::Destructure(items, _) => items
            .iter()
            .filter_map(|item| item.target.as_ref().and_then(property_of))
            .max(),
        ExprKind::PropertyAccess(base, _, _) | ExprKind::DynamicPropertyAccess(base, _, _) => {
            Some(own_or_other(base))
        }
        ExprKind::Index(base, _) => property_of_index_base(base),
        _ => None,
    }
}

fn property_of_index_base(base: &Expr) -> Option<PropertyOf> {
    match &base.peeled().kind {
        ExprKind::PropertyAccess(object, _, _) | ExprKind::DynamicPropertyAccess(object, _, _) => {
            Some(own_or_other(object))
        }
        ExprKind::Index(inner, _) => property_of_index_base(inner),
        _ => None,
    }
}

fn own_or_other(object: &Expr) -> PropertyOf {
    if matches!(&object.peeled().kind, ExprKind::Variable(v) if **v == *"this") {
        PropertyOf::This
    } else {
        PropertyOf::Other
    }
}

/// `$obj->prop = ...`/`unset($obj->prop)`: a write to a property of any
/// object, through any depth of array-index nesting.
fn property_write_target(e: &Expr) -> bool {
    stack::guard(|| match &e.kind {
        ExprKind::PropertyAccess(..) | ExprKind::DynamicPropertyAccess(..) => true,
        ExprKind::Index(base, _) => property_write_target(base),
        _ => false,
    })
}

impl Summarizer {
    fn receiver(&self, recv: &Expr) -> Option<Receiver> {
        match &recv.peeled().kind {
            ExprKind::Variable(v) if **v == *"this" => Some(Receiver::This),
            ExprKind::Variable(v) => Some(Receiver::Variable(v.clone())),
            ExprKind::PropertyAccess(base, prop, _) if matches!(&base.peeled().kind, ExprKind::Variable(v) if **v == *"this") => {
                Some(Receiver::ThisProperty(prop.name.clone()))
            }
            _ => None,
        }
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
        stack::guard(|| {
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
                        self.write(target);
                    }
                }
                StmtKind::Foreach {
                    subject,
                    key,
                    value,
                    by_ref,
                    ..
                } => {
                    self.write(value);
                    if *by_ref {
                        self.write(subject);
                    }
                    for target in key.iter().map(|k| &**k).chain([&**value]) {
                        collect_assigned(target, &mut self.reassigned);
                    }
                }
                _ => {}
            }
            walk_stmt(self, stmt);
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        stack::guard(|| {
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
                ExprKind::Assign(lhs, _) | ExprKind::CompoundAssign(_, lhs, _) => {
                    self.write(lhs);
                    collect_assigned(lhs, &mut self.reassigned);
                }
                ExprKind::AssignRef(lhs, rhs) => {
                    self.write(lhs);
                    self.write(rhs);
                    collect_assigned(lhs, &mut self.reassigned);
                }
                ExprKind::Unary(
                    UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                    target,
                ) => self.write(target),
                ExprKind::Eval(_) | ExprKind::Include(..) => self.anything = true,
                ExprKind::Call(callee, args) => match &callee.kind {
                    ExprKind::Name(name) => {
                        self.purity_call(PurityCall::Function(name.clone()));
                        self.call(Callee::Function(name.clone()), args);
                    }
                    _ => {
                        self.impure_unless_in_closure(true);
                        self.anything = true;
                    }
                },
                ExprKind::MethodCall(recv, method, args, _) => {
                    let on_this = matches!(&recv.kind, ExprKind::Variable(v) if *v == "this");
                    if on_this {
                        self.purity_call(PurityCall::ThisMethod(method.name.clone()));
                    } else {
                        self.impure_unless_in_closure(true);
                    }
                    match self.receiver(recv) {
                        Some(receiver) => self.call(
                            Callee::Method {
                                receiver,
                                method: method.name.clone(),
                            },
                            args,
                        ),
                        None => self.anything = true,
                    }
                }
                ExprKind::StaticCall(target, method, args) => match target.as_name() {
                    Some(name) => {
                        self.purity_call(PurityCall::Static(name.clone(), method.name.clone()));
                        self.call(Callee::Static(name.clone(), method.name.clone()), args);
                    }
                    None => {
                        self.impure_unless_in_closure(true);
                        self.anything = true;
                    }
                },
                ExprKind::DynamicMethodCall(..) | ExprKind::DynamicStaticCall(..) => {
                    self.anything = true;
                }
                ExprKind::New(callee, args) => match &callee.kind {
                    ExprKind::Name(name) => {
                        self.purity_call(PurityCall::New(name.clone()));
                        self.call(Callee::New(name.clone()), args);
                    }
                    _ => {
                        self.impure_unless_in_closure(true);
                        self.anything = true;
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
