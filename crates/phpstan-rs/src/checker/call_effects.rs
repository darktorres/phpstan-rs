//! What a call may have changed, forgotten after it: the storages its
//! effect (`registry::effects`) says it may write, matched to what the
//! scope knows of them. A property's facts go on each value whose type may
//! hold an object the call writes, except an object the scope made that
//! never leaves it (`SharedScope::unescaped`); a static property's by the
//! class declaring it; and a variable bound to a written global, static
//! variable or place, or captured by reference by a closure the call may
//! run, is reset to what it may hold.

use std::rc::Rc;

use syntax::ast::*;
use syntax::name::Name;
use types::Type;

use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::shared_scope::SharedScope;
use crate::checker::{Checker, Locals, purge_stale_narrowing_for, purge_stale_narrowing_for_var};
use crate::registry::summaries::{
    ArgAt, BodySummary, Callable, PairTarget, Place, Receiver, Root, Step, call_of,
};
use crate::registry::{CallContext, Effect, Holder, Storage, holder_of};

/// What a call's caller knows of it, from the types the checker inferred.
struct TypedCall<'c, 'a> {
    checker: &'c Checker<'a>,
    locals: &'c Locals,
    receiver: Option<&'c Expr>,
    args: &'c [Arg],
}

impl TypedCall<'_, '_> {
    /// The argument passed `at`.
    fn arg(&self, at: &ArgAt) -> Option<&Expr> {
        match at {
            ArgAt::Position(at) => self.args.get(*at as usize).map(|arg| &arg.value),
            ArgAt::Named(name) => self
                .args
                .iter()
                .find(|arg| arg.name.as_ref().is_some_and(|n| n.name == *name))
                .map(|arg| &arg.value),
            ArgAt::Spread => self
                .args
                .iter()
                .find(|arg| arg.spread)
                .map(|arg| &arg.value),
        }
    }

    fn inferred(&self, e: &Expr) -> Holder {
        holder_of(&self.checker.infer_expr(e, self.locals))
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
        self.arg(at)
            .map_or(Holder::Unknown, |arg| self.inferred(arg))
    }

    fn written_effect(&self, at: &ArgAt, written: bool, place: Option<&Place>) -> Option<Effect> {
        if written {
            let summary = match &self.arg(at)?.peeled().kind {
                ExprKind::Closure(closure) => BodySummary::of(&closure.body),
                ExprKind::ArrowFn(arrow) => BodySummary::of_expr(&arrow.body),
                ExprKind::FirstClassCallable(call) => BodySummary::of_expr(call),
                _ => return None,
            };
            return Some(self.checker.registry.body_effect(&summary, self.class()));
        }
        match place.map(|place| (&place.root, &*place.steps)) {
            Some((Root::Variable(Some(name)), [])) => self.called_effect(name),
            _ => None,
        }
    }

    fn called_effect(&self, name: &Name) -> Option<Effect> {
        let bodies = self.checker.shared_scope.closures.get(name)?;
        let mut effect = Effect::nothing();
        for body in bodies {
            effect.join(&self.checker.registry.body_effect(body, self.class()));
            if effect.is_anything() {
                break;
            }
        }
        Some(effect)
    }
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
    /// doc comment.
    pub(crate) fn forget_what_a_call_may_change(&mut self, e: &Expr, locals: &mut Locals) {
        let (receiver, args) = match &e.kind {
            ExprKind::MethodCall(recv, _, args, _)
            | ExprKind::DynamicMethodCall(recv, _, args, _) => (Some(&**recv), &args[..]),
            ExprKind::Call(_, args)
            | ExprKind::New(_, args)
            | ExprKind::StaticCall(_, _, args)
            | ExprKind::DynamicStaticCall(_, _, args) => (None, &args[..]),
            ExprKind::NewAnonymousClass(class) => (None, &class.args[..]),
            _ => return,
        };
        let scope = Rc::clone(&self.shared_scope);
        if !locals.has_state_to_forget() && scope.binds_nothing() {
            return;
        }
        let Some(mut call) = call_of(e) else {
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
        let effect = {
            let ctx = TypedCall {
                checker: self,
                locals,
                receiver,
                args,
            };
            self.registry.call_effect(&call, &ctx)
        };
        // A closure the scope wrote writes what it captures by reference as
        // its own variables, which no effect names.
        let runs_written = effect.is_anything()
            || args.iter().any(|arg| match &arg.value.peeled().kind {
                ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
                    true
                }
                ExprKind::Variable(name) => scope.closures.contains_key(name),
                _ => false,
            })
            || matches!(&e.kind, ExprKind::Call(callee, _)
                if matches!(&callee.peeled().kind, ExprKind::Variable(name) if scope.closures.contains_key(name)));
        self.forget_effect(&effect, runs_written, locals);
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
        for key in stale {
            purge_stale_narrowing_for(locals, key);
        }
        for (name, shared) in
            self.shared_variables_written(&storages, anything, runs_written, &scope, locals)
        {
            let ty = match shared {
                Shared::Global(global) => self
                    .registry
                    .global_variables
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
            own.and_then(|own| self.registry.classes.get(own))
                .and_then(|info| info.extends().first().map(|parent| parent.to_string()))
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
                self.registry.global_variables.get(name.as_str()).cloned()
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
