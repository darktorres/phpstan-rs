//! What a call may have changed, forgotten after it: the storages its
//! effect (`registry::effects`) says it may write, matched to what the
//! scope knows of them. An operation PHP may run a value's code for (a
//! magic method, an `ArrayAccess` or an iterator's method) is such a call.
//! A property's facts go on each value whose type may hold an object the
//! call writes, except an object the scope made that never leaves it
//! (`SharedScope::unescaped`); a static property's by the class declaring
//! it; and a variable bound to a written global, static variable or place,
//! or captured by reference by a closure the call may run, is reset to what
//! it may hold.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use syntax::ast::*;
use syntax::name::Name;
use types::Type;

use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::shared_scope::SharedScope;
use crate::checker::{Checker, Locals, purge_stale_narrowing_for, purge_stale_narrowing_for_var};
use crate::registry::summaries::{
    ArgAt, BodySummary, CallWrites, Callable, Callee, Operation, Origin, PairTarget, Place,
    Receiver, Root, Step, call_of,
};
use crate::registry::{
    CallContext, Effect, Holder, Storage, Transfer, Written, elements_holder_of, holder_of,
};

/// What a call's caller knows of it, from the types the checker inferred.
struct TypedCall<'c, 'a> {
    checker: &'c Checker<'a>,
    locals: &'c Locals,
    /// What a function call calls, when not a name.
    callee: Option<&'c ExprNode>,
    receiver: Option<&'c ExprNode>,
    arenas: Option<&'c NodeArenas>,
    args: &'c [Arg],
    /// What the closures the scope wrote that the call runs may write.
    written: RefCell<Effect>,
    /// Whether the call runs a closure the scope wrote.
    runs_written: Cell<bool>,
    /// The object an operation's operand is, and whether it is the scope's
    /// own `$this`, for an operation PHP may run code for.
    operand: Option<(Holder, bool)>,
}

impl TypedCall<'_, '_> {
    /// The argument passed `at`.
    fn arg(&self, at: &ArgAt) -> Option<&ExprNode> {
        let arg = match at {
            ArgAt::Position(at) => self.args.get(*at as usize),
            ArgAt::Named(name) => self
                .args
                .iter()
                .find(|arg| arg.name.as_ref().is_some_and(|n| n.name == *name)),
            ArgAt::Spread => self.args.iter().find(|arg| arg.spread),
        }?;
        self.arenas?.expr(arg.value)
    }

    fn inferred(&self, e: &ExprNode) -> Holder {
        holder_of(&self.checker.infer_expr(e, self.locals))
    }

    /// What the scope knows of the object the variable `name` holds, for a
    /// closure that captures it by value.
    fn captured_holder(&self) -> impl Fn(&str) -> Option<Holder> + '_ {
        |name| self.locals.get_var(name).map(holder_of)
    }

    /// What running `e` may write, when it is a closure, an arrow function
    /// or a first-class callable, there or assigned there.
    fn closure_effect(&self, e: &ExprNode) -> Option<Effect> {
        let arenas = self.checker.syntax_arenas()?;
        let mut id = arenas.peeled(e.id)?;
        loop {
            match &arenas.expr(id)?.kind {
                ExprKind::Assign(_, rhs) => id = arenas.peeled(*rhs)?,
                _ => break,
            }
        }
        matches!(
            &arenas.expr(id)?.kind,
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_)
        )
        .then(|| {
            let summary = BodySummary::of_closure(arenas, id);
            self.checker.registry.closure_effect(
                &summary,
                self.class(),
                Some(&self.captured_holder()),
            )
        })
    }

    /// What calling the variable `name` may write, when it holds only
    /// closures the scope wrote.
    fn held_effect(&self, name: &Name) -> Option<Effect> {
        let closures = self.checker.shared_scope.closures.get(name)?;
        let mut effect = Effect::nothing();
        for closure in closures {
            effect.join(&self.checker.registry.closure_effect(
                closure,
                self.class(),
                Some(&self.captured_holder()),
            ));
            if effect.is_anything() {
                break;
            }
        }
        Some(effect)
    }
}

impl CallContext for TypedCall<'_, '_> {
    fn class(&self) -> Option<&str> {
        self.checker.current_class.as_deref()
    }

    fn receiver(&self, receiver: &Receiver) -> (Holder, bool) {
        match receiver {
            Receiver::This => (
                match self.class() {
                    Some(class) => Holder::Classes(vec![Name::from(class)]),
                    None => Holder::Unknown,
                },
                true,
            ),
            Receiver::Variable(name) => (
                self.locals.get_var(name).map_or(Holder::Unknown, holder_of),
                false,
            ),
            Receiver::ThisProperty(prop) => {
                let narrowed = self
                    .locals
                    .find_var("this")
                    .and_then(|this| {
                        self.locals.find(&KeyData::Prop {
                            base: this,
                            prop: prop.clone(),
                        })
                    })
                    .and_then(|key| self.locals.get(key))
                    .cloned();
                let ty = narrowed.or_else(|| {
                    self.class()
                        .and_then(|class| self.checker.registry.lookup_property(class, prop))
                        .map(|p| p.ty().clone())
                });
                (ty.map_or(Holder::Unknown, |ty| holder_of(&ty)), false)
            }
            // The call's own receiver: the only one of these kinds a call the
            // checker asks about has (`forget_what_a_call_may_change`).
            Receiver::Static(..) | Receiver::Unknown => (
                self.receiver
                    .map_or(Holder::Unknown, |receiver| self.inferred(receiver)),
                false,
            ),
        }
    }

    fn argument(&self, at: &ArgAt, _: Option<&Place>) -> Holder {
        // A spread argument is its array's elements.
        if matches!(at, ArgAt::Spread) {
            return self.elements(at, None);
        }
        self.arg(at)
            .map_or(Holder::Unknown, |arg| self.inferred(arg))
    }

    fn elements(&self, at: &ArgAt, _: Option<&Place>) -> Holder {
        self.arg(at).map_or(Holder::Unknown, |arg| {
            elements_holder_of(&self.checker.infer_expr(arg, self.locals))
        })
    }

    fn runs_written(&self, written: Written<'_>, transfer: &Transfer) -> bool {
        let effect = match written {
            Written::Argument {
                at,
                callable: Some(Callable::Written(_)),
                ..
            } => self.arg(at).and_then(|arg| self.closure_effect(arg)),
            Written::Argument {
                callable: None,
                place: Some(place),
                ..
            } => match (&place.root, &*place.steps) {
                (Root::Variable(Some(name)), []) => self.held_effect(name),
                _ => None,
            },
            Written::Argument { .. } => None,
            Written::Variable(name) => self.held_effect(name),
            Written::Callee(_) => self.callee.and_then(|callee| self.closure_effect(callee)),
        };
        let Some(effect) = effect else {
            return false;
        };
        self.written
            .borrow_mut()
            .join(&effect.transferred(transfer));
        self.runs_written.set(true);
        true
    }

    fn operand(&self, _: Option<&[Origin]>) -> (Holder, bool) {
        self.operand.clone().unwrap_or((Holder::Unknown, false))
    }

    fn rebindable(&self) -> bool {
        self.checker.shared_scope.closure
    }
}

/// Whether `e` is the scope's own `$this`.
pub(crate) fn is_this(checker: &Checker<'_>, e: &ExprNode) -> bool {
    checker
        .syntax_arenas()
        .and_then(|arenas| arenas.peeled(e.id).and_then(|id| arenas.expr(id)))
        .is_some_and(|e| matches!(&e.kind, ExprKind::Variable(name) if **name == *"this"))
}

/// How a shared variable is reset.
enum Shared {
    /// To what the global of this name may hold.
    Global(Name),
    /// To anything: a static variable.
    Anything,
    /// To what the place it refers to may hold, by its declaration.
    Reference(Place),
    /// To what it holds and what a closure capturing it may have written.
    Captured,
}

impl Checker<'_> {
    /// After a call, what it may have written is stale: see the module's
    /// doc comment. So is what iterating an argument spread into it may
    /// have written.
    pub(crate) fn forget_what_a_call_may_change(&mut self, e: &ExprNode, locals: &mut Locals) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let (callee, receiver, args) = match &e.kind {
            ExprKind::MethodCall(recv, _, args, _)
            | ExprKind::DynamicMethodCall(recv, _, args, _) => {
                (None, arenas.expr(*recv), arenas.args(args.clone()))
            }
            ExprKind::Call(callee, args) => (arenas.expr(*callee), None, arenas.args(args.clone())),
            ExprKind::New(_, args)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicStaticCall(_, _, args) => (None, None, arenas.args(args.clone())),
            ExprKind::NewAnonymousClass(class_id) => {
                let Some(class) = arenas.anonymous_class(*class_id) else {
                    return;
                };
                (None, None, arenas.args(class.args.clone()))
            }
            _ => return,
        };
        for arg in args.iter().filter(|arg| arg.spread) {
            if let Some(value) = arenas.expr(arg.value) {
                self.forget_what_an_operation_on_may_change(
                    value,
                    Operation::Iterate { keys: true },
                    locals,
                );
            }
        }
        let scope = Rc::clone(&self.shared_scope);
        if !locals.has_state_to_forget() && scope.binds_nothing() {
            return;
        }
        let Some(mut call) = call_of(arenas, e.id) else {
            return;
        };
        // An array callable on a value the context cannot name is not
        // followed: it may run anything.
        call.callables = call
            .callables
            .iter()
            .filter(|(_, callable)| {
                !matches!(
                    callable,
                    Callable::Pair(
                        PairTarget::Object(Receiver::Static(..) | Receiver::Unknown),
                        _
                    )
                )
            })
            .cloned()
            .collect();
        let (effect, ran_written) = {
            let ctx = TypedCall {
                checker: self,
                locals,
                callee,
                receiver,
                arenas: Some(arenas),
                args,
                written: Default::default(),
                runs_written: Cell::new(false),
                operand: None,
            };
            let mut effect = self.registry.call_effect(&call, &ctx);
            effect.join(&ctx.written.borrow());
            (effect, ctx.runs_written.get())
        };
        // A closure the scope wrote writes what it captures by reference as
        // its own variables, which no effect names.
        let runs_written = effect.is_anything() || ran_written;
        self.forget_effect(&effect, runs_written, locals);
    }

    /// After `operation` on a value of type `ty`, what the code PHP may run
    /// of the value for it may have written is stale; `this` when the value
    /// is the scope's own `$this`.
    pub(crate) fn forget_what_an_operation_may_change(
        &self,
        ty: &Type,
        this: bool,
        operation: Operation,
        locals: &mut Locals,
    ) {
        if !locals.has_state_to_forget() && self.shared_scope.binds_nothing() {
            return;
        }
        if ty.is_object().no() {
            return;
        }
        let call = CallWrites {
            callee: Callee::Implicit {
                on: None,
                operation,
            },
            arguments: Box::new([]),
            places: Box::new([]),
            objectless: Box::new([]),
            callables: Box::new([]),
        };
        let effect = {
            let ctx = TypedCall {
                checker: self,
                locals,
                callee: None,
                receiver: None,
                arenas: self.syntax_arenas(),
                args: &[],
                written: Default::default(),
                runs_written: Cell::new(false),
                operand: Some((holder_of(ty), this)),
            };
            self.registry.call_effect(&call, &ctx)
        };
        self.forget_effect(&effect, effect.is_anything(), locals);
    }

    /// After the binary operator `op` on `l` and `r`, each with its type,
    /// what converting an operand to a string may have written is stale:
    /// both operands of `.`, and an operand compared loosely with one that
    /// may be a string.
    pub(crate) fn binary_string_conversions(
        &self,
        op: &BinOp,
        l: (&ExprNode, &Type),
        r: (&ExprNode, &Type),
        locals: &mut Locals,
    ) {
        let converted = match op {
            BinOp::Concat => [true, true],
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::LtEq
            | BinOp::GtEq
            | BinOp::Spaceship => [!r.1.is_string().no(), !l.1.is_string().no()],
            _ => return,
        };
        for ((operand, ty), converted) in [l, r].into_iter().zip(converted) {
            if converted {
                self.forget_what_an_operation_may_change(
                    ty,
                    is_this(self, operand),
                    Operation::String,
                    locals,
                );
            }
        }
    }

    /// `forget_what_an_operation_may_change` for `operation` on the value of
    /// `operand`, evaluated.
    pub(crate) fn forget_what_an_operation_on_may_change(
        &self,
        operand: &ExprNode,
        operation: Operation,
        locals: &mut Locals,
    ) {
        if !locals.has_state_to_forget() && self.shared_scope.binds_nothing() {
            return;
        }
        let ty = self.infer_expr(operand, locals);
        self.forget_what_an_operation_may_change(&ty, is_this(self, operand), operation, locals);
    }

    /// Forgets what `effect` may have written, and the variables captured by
    /// reference when the call runs a closure the scope wrote.
    pub(crate) fn forget_effect(&self, effect: &Effect, runs_written: bool, locals: &mut Locals) {
        let scope = Rc::clone(&self.shared_scope);
        if effect.is_nothing() && !runs_written {
            return;
        }
        let anything = effect.is_anything();
        let storages: Vec<&Storage> = effect.storages().into_iter().flatten().collect();
        let mut stale: Vec<LocalKey> = Vec::new();
        let keys: Vec<LocalKey> = locals.synthetic_keys().copied().collect();
        for key in keys {
            let written = match locals.data(key) {
                KeyData::Prop { base, prop } => {
                    self.object_written(&storages, anything, base, Some(&prop), &scope, locals)
                }
                KeyData::Index { base, .. } | KeyData::VarIndex { base, .. } => {
                    self.object_written(&storages, anything, base, None, &scope, locals)
                }
                KeyData::StaticProp { class, prop } => {
                    anything || self.static_written(&storages, &class, &prop)
                }
                KeyData::Var(_) | KeyData::Truthy(_) | KeyData::Isset(_) => false,
            };
            if written {
                stale.push(key);
            }
        }
        // A written place's fact goes, and with it every fact that rests on
        // it; a read then gives what its declaration allows.
        for key in stale {
            purge_stale_narrowing_for(locals, key);
            locals.remove(key);
        }
        for (name, shared) in
            self.shared_variables_written(&storages, anything, runs_written, &scope, locals)
        {
            let ty = match shared {
                Shared::Global(global) => self
                    .registry
                    .global_variables()
                    .get(global.as_str())
                    .cloned()
                    .unwrap_or_else(Type::mixed),
                Shared::Anything => Type::mixed(),
                Shared::Reference(place) => self.place_type(&place),
                Shared::Captured => match (locals.get_var(&name), self.captured_types.get(&name)) {
                    (Some(held), Some(written)) => self
                        .relation()
                        .union_of(vec![held.clone(), written.clone()]),
                    _ => Type::mixed(),
                },
            };
            if !locals.contains_var(&name) {
                continue;
            }
            purge_stale_narrowing_for_var(locals, &name);
            locals.insert_var(&name, ty);
            locals.clear_native_var(&name);
        }
    }

    /// Whether a property `prop` (any, when `None`, or an element) of the
    /// value at `base` may be among what `storages` say is written.
    fn object_written(
        &self,
        storages: &[&Storage],
        anything: bool,
        base: LocalKey,
        prop: Option<&Name>,
        scope: &SharedScope,
        locals: &Locals,
    ) -> bool {
        if let KeyData::Var(name) = root_variable(base, locals)
            && scope.unescaped.contains(&name)
        {
            return false;
        }
        if prop.is_none() && self.private_array(base, scope, locals) {
            return false;
        }
        if let Some(prop) = prop
            && self.readonly_property(base, prop, locals)
        {
            return false;
        }
        if anything {
            return true;
        }
        let base_ty = self.key_type(base, locals);
        let names = |written: &Option<Name>| match (written, prop) {
            (None, _) | (_, None) => true,
            (Some(written), Some(prop)) => written == prop,
        };
        storages.iter().any(|storage| match storage {
            Storage::Own(written) => {
                names(written) && self.may_hold(&base_ty, self.current_class.as_deref())
            }
            Storage::Property {
                class,
                prop: written,
            } => names(written) && self.may_hold(&base_ty, class.as_deref()),
            Storage::Static { .. } | Storage::Global(_) | Storage::StaticVar { .. } => false,
        })
    }

    /// Whether `prop` is a native `readonly` property of every class the
    /// value at `base` may be: once initialised it is written by no code, so
    /// what the scope knows of it holds through any call.
    fn readonly_property(&self, base: LocalKey, prop: &Name, locals: &Locals) -> bool {
        let Some(ty) = self.key_type(base, locals) else {
            return false;
        };
        if !ty.is_object().yes() {
            return false;
        }
        let classes = ty.object_class_names();
        !classes.is_empty()
            && classes.iter().all(|class| {
                self.registry
                    .lookup_property(class, prop)
                    .is_some_and(|declared| {
                        declared.is_readonly() && !declared.readonly_by_phpdoc()
                    })
            })
    }

    /// Whether the element facts of the value at `base` rest on arrays and
    /// strings held by a variable no other code can reach: a value no call
    /// writes, unless a reference to the variable or to an element of it was
    /// taken.
    fn private_array(&self, base: LocalKey, scope: &SharedScope, locals: &Locals) -> bool {
        let mut key = base;
        loop {
            let Some(ty) = self.key_type(key, locals) else {
                return false;
            };
            if !ty
                .union_members()
                .iter()
                .all(|member| member.is_array().yes() || member.is_string().yes())
            {
                return false;
            }
            match locals.data(key) {
                KeyData::Var(name) => {
                    return scope.is_private(&name) && !locals.exposed().contains(&name);
                }
                KeyData::Index { base, .. } | KeyData::VarIndex { base, .. } => key = base,
                _ => return false,
            }
        }
    }

    /// Whether `storages` may write the static property `prop` named through
    /// `class` (`static` and `parent` as written).
    fn static_written(&self, storages: &[&Storage], class: &Name, prop: &Name) -> bool {
        let own = self.current_class.as_deref();
        let classes: Vec<String> = if class.eq_ignore_ascii_case("static") {
            own.map(|own| {
                std::iter::once(own.to_string())
                    .chain(self.registry.subclasses(own).iter().map(|c| c.to_string()))
                    .collect()
            })
            .unwrap_or_default()
        } else if class.eq_ignore_ascii_case("parent") {
            own.and_then(|own| self.registry.class_references_by_name(own))
                .and_then(|references| references.extends.first().map(|parent| parent.to_string()))
                .into_iter()
                .collect()
        } else {
            vec![class.to_string()]
        };
        storages.iter().any(|storage| {
            let Storage::Static {
                class: written_class,
                prop: written_prop,
            } = storage
            else {
                return false;
            };
            if written_prop.as_ref().is_some_and(|written| written != prop) {
                return false;
            }
            let Some(written_class) = written_class else {
                return true;
            };
            classes.is_empty()
                || classes.iter().any(|class| {
                    match self.registry.lookup_property_with_owner(class, prop) {
                        Some((owner, _)) => owner.eq_ignore_ascii_case(written_class),
                        None => true,
                    }
                })
        })
    }

    /// The scope's shared variables `storages` may write, and how each is
    /// reset.
    fn shared_variables_written(
        &self,
        storages: &[&Storage],
        anything: bool,
        runs_written: bool,
        scope: &SharedScope,
        locals: &Locals,
    ) -> Vec<(Name, Shared)> {
        let mut out: Vec<(Name, Shared)> = Vec::new();
        let global_written = |name: &Name| {
            anything
                || storages.iter().any(|storage| match storage {
                    Storage::Global(None) => true,
                    Storage::Global(Some(global)) => global == name,
                    _ => false,
                })
        };
        for name in &scope.globals {
            if global_written(name) {
                out.push((name.clone(), Shared::Global(name.clone())));
            }
        }
        // Every variable is a global at a file's top level, and may be one
        // after a `global $$n`; the superglobals are everywhere.
        for (name, _) in locals.variables() {
            let global = scope.top_level
                || scope.any_global
                || (is_superglobal(&name) && *name != *"GLOBALS");
            if global && global_written(&name) {
                out.push((name.clone(), Shared::Global(name)));
            }
        }
        let own_function = |class: &Option<Name>, function: &Name| {
            scope
                .function
                .as_ref()
                .is_some_and(|(own_class, own_function)| {
                    own_function.eq_ignore_ascii_case(function)
                        && match (own_class, class) {
                            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                            (None, None) => true,
                            _ => false,
                        }
                })
        };
        for name in &scope.statics {
            let written = anything
                || storages.iter().any(|storage| match storage {
                    Storage::StaticVar {
                        class,
                        function,
                        var,
                    } => {
                        own_function(class, function) && var.as_ref().is_none_or(|var| var == name)
                    }
                    _ => false,
                });
            if written {
                out.push((name.clone(), Shared::Anything));
            }
        }
        for (name, place) in &scope.references {
            if anything || self.place_written(place, storages) {
                out.push((name.clone(), Shared::Reference(place.clone())));
            }
        }
        if runs_written {
            for name in &scope.captured {
                out.push((name.clone(), Shared::Captured));
            }
        }
        out
    }

    /// What the place a reference was taken of may hold, by its
    /// declaration: a global's, a static property's, a property of
    /// `$this`'s; anything else's anything.
    fn place_type(&self, place: &Place) -> Type {
        let own = self.current_class.as_deref();
        let declared = match (&place.root, &*place.steps) {
            (Root::Global(Some(name)), []) => {
                self.registry.global_variables().get(name.as_str()).cloned()
            }
            (Root::Static(class, Some(prop)), []) => {
                let class = if class.is_keyword("self")
                    || class.is_keyword("static")
                    || class.is_keyword("parent")
                {
                    own.map(str::to_string)
                } else {
                    class.resolved_class().map(str::to_string)
                };
                class
                    .and_then(|class| self.registry.lookup_property(&class, prop))
                    .map(|p| p.ty().clone())
            }
            (Root::This, [Step::Property(Some(prop))]) => own
                .and_then(|class| self.registry.lookup_property(class, prop))
                .map(|p| p.ty().clone()),
            _ => None,
        };
        declared.unwrap_or_else(Type::mixed)
    }

    /// Whether `storages` may write the place a reference was taken of.
    fn place_written(&self, place: &Place, storages: &[&Storage]) -> bool {
        let last_property = place.steps.iter().rev().find_map(|step| match step {
            Step::Property(prop) => Some(prop.clone()),
            Step::Element => None,
        });
        storages
            .iter()
            .any(|storage| match (storage, &place.root, &last_property) {
                (Storage::Own(_) | Storage::Property { .. }, _, Some(_)) => true,
                (Storage::Global(written), Root::Global(name), None) => {
                    written.is_none() || name.is_none() || written == name
                }
                (Storage::Static { prop: written, .. }, Root::Static(_, prop), None) => {
                    written.is_none() || prop.is_none() || written == prop
                }
                (Storage::Static { .. }, Root::DynamicStatic(_), None) => true,
                _ => false,
            })
    }

    /// The type of the value at `key`: what is known of it, or what its
    /// declaration says.
    fn key_type(&self, key: LocalKey, locals: &Locals) -> Option<Type> {
        if let Some(ty) = locals.get(key) {
            return Some(ty.clone());
        }
        syntax::stack::guard(|| match locals.data(key) {
            KeyData::Var(name) if *name == *"this" => self
                .current_class
                .as_deref()
                .map(|class| Type::object(Name::from(class), Vec::new())),
            KeyData::Prop { base, prop } => {
                let base_ty = self.key_type(base, locals)?;
                let mut types = Vec::new();
                for member in base_ty.union_members() {
                    let (class, _) = member.as_object()?;
                    types.push(self.registry.lookup_property(class, &prop)?.ty().clone());
                }
                Some(self.relation().union_of(types))
            }
            KeyData::Index { base, .. } | KeyData::VarIndex { base, .. } => {
                self.key_type(base, locals)?.array_value_type()
            }
            KeyData::StaticProp { class, prop } => {
                let class = if class.eq_ignore_ascii_case("static")
                    || class.eq_ignore_ascii_case("parent")
                {
                    self.current_class.clone()?
                } else {
                    class.to_string()
                };
                Some(self.registry.lookup_property(&class, &prop)?.ty().clone())
            }
            KeyData::Var(_) | KeyData::Truthy(_) | KeyData::Isset(_) => None,
        })
    }

    /// Whether a value of type `ty` (any, when not known) may be an object
    /// of `class` (of any class, when `None`).
    fn may_hold(&self, ty: &Option<Type>, class: Option<&str>) -> bool {
        let Some(ty) = ty else {
            return true;
        };
        if ty.is_object().no() {
            return false;
        }
        match class {
            None => true,
            Some(class) => !self
                .relation()
                .intersect(vec![
                    ty.clone(),
                    Type::object(Name::from(class), Vec::new()),
                ])
                .is_never(),
        }
    }
}

/// The variable a key starts from.
fn root_variable(key: LocalKey, locals: &Locals) -> KeyData {
    let mut key = key;
    loop {
        match locals.data(key) {
            KeyData::Prop { base, .. }
            | KeyData::Index { base, .. }
            | KeyData::VarIndex { base, .. } => key = base,
            KeyData::Truthy(inner) | KeyData::Isset(inner) => key = inner,
            data @ (KeyData::Var(_) | KeyData::StaticProp { .. }) => return data,
        }
    }
}
