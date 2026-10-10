//! What a call may have changed, forgotten after it: the storages its
//! effect (`registry::effects`) says it may write, matched to what the
//! scope knows of them. An operation PHP may run a value's code for (a
//! magic method, an `ArrayAccess` or an iterator's method) is such a call.
//! A property's facts go on each value whose type may hold an object the
//! call writes, except an object the scope made that never leaves it
//! (`SharedScope::made`); a static property's by the class declaring
//! it; and a variable bound to a written global, static variable or place,
//! or captured by reference by a closure the call may run, is reset to what
//! it may hold.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};
use types::Type;

use crate::checker::keys::{KeyData, LocalKey};
use crate::checker::shared_scope::SharedScope;
use crate::checker::{Checker, Locals, purge_stale_narrowing_for, purge_stale_narrowing_for_var};
use crate::file_id::FileId;
use crate::registry::summaries::{
    ArgAt, BodySummary, CallWrites, Callable, Callee, Operation, Origin, PairTarget, Place,
    Receiver, Root, Step, call_of,
};
use crate::registry::{
    CallContext, Effect, Holder, Storage, Transfer, Written, elements_holder_of, holder_of,
};

/// The superglobals every scope holds, `$GLOBALS` apart.
const SUPERGLOBALS_BUT_GLOBALS: [&str; 10] = [
    "_GET", "_POST", "_REQUEST", "_SERVER", "_SESSION", "_COOKIE", "_FILES", "_ENV", "argv", "argc",
];

/// What an operation on a value may write, by the operation, the value's
/// holder and whether it is the scope's own `$this`, for the class, file and
/// kind of function-like the answers were made in.
#[derive(Default)]
pub(crate) struct OperationEffects {
    scope: Option<(Option<String>, Option<FileId>, bool)>,
    answers: rustc_hash::FxHashMap<(Operation, Holder, bool), Effect>,
}

impl OperationEffects {
    /// The answers, emptied when they were made in another scope.
    fn in_scope(&mut self, class: Option<&str>, file: Option<FileId>, rebindable: bool) {
        let same = self
            .scope
            .as_ref()
            .is_some_and(|(c, f, r)| c.as_deref() == class && *f == file && *r == rebindable);
        if !same {
            self.answers.clear();
            self.scope = Some((class.map(str::to_owned), file, rebindable));
        }
    }
}

fn effect_memo_enabled() -> bool {
    static ENABLED: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("PHPSTAN_RS_NO_EFFECT_MEMO").is_none());
    *ENABLED
}

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
    /// The object an operation's operand is, and whether it is the scope's
    /// own `$this`, for an operation PHP may run code for.
    operand: Option<(Holder, bool)>,
    /// What the by-reference arguments were before the call replaced them
    /// with the parameters' types, by where they are.
    before: rustc_hash::FxHashMap<syntax::span::Span, Type>,
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
        holder_of(&self.type_of(e))
    }

    /// The type of the argument `e`, before the call.
    fn type_of(&self, e: &ExprNode) -> Type {
        match self.before.get(&self.checker.expr_span(e.id)) {
            Some(before) => before.clone(),
            None => self.checker.infer_expr(e, self.locals),
        }
    }

    /// What the scope knows of the object the variable `name` holds, for a
    /// closure written where the call is that captures it by value.
    fn captured_holder(&self) -> impl Fn(&str) -> Option<Holder> + '_ {
        |name| self.locals.get_var(name).map(holder_of)
    }

    /// What running `e` may write, when it is a closure, an arrow function
    /// or a first-class callable, there or assigned there.
    fn closure_effect(&self, e: &ExprNode) -> Option<Effect> {
        let arenas = self.checker.syntax_arenas()?;
        let mut id = arenas.peeled(e.id)?;
        while let ExprKind::Assign(_, rhs) = &arenas.expr(id)?.kind {
            id = arenas.peeled(*rhs)?;
        }
        matches!(
            &arenas.expr(id)?.kind,
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_)
        )
        .then(|| {
            let summary = BodySummary::of_closure(arenas, id);
            let mut effect = self.checker.registry.closure_effect(
                &summary,
                self.class(),
                self.checker.current_file_id,
                Some(&self.captured_holder()),
            );
            effect.join(&captured_effect(arenas, id));
            effect
        })
    }

    /// What calling the variable `name` may write, when it holds only
    /// closures the scope wrote.
    fn held_effect(&self, name: &Name) -> Option<Effect> {
        let closures = self.checker.shared_scope.closures.get(name)?;
        let mut effect = Effect::nothing();
        for (id, closure) in closures {
            // What the closure took where it was written, not what the
            // variables hold now.
            let captured = self.checker.closure_captures.get(id);
            let taken = |name: &str| captured?.get(&Name::from(name)).cloned();
            effect.join(&self.checker.registry.closure_effect(
                closure,
                self.class(),
                self.checker.current_file_id,
                Some(&taken),
            ));
            if let Some(arenas) = self.checker.syntax_arenas() {
                effect.join(&captured_effect(arenas, *id));
            }
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

    fn file(&self) -> Option<FileId> {
        self.checker.current_file_id
    }

    fn receiver(&self, receiver: &Receiver) -> (Holder, bool) {
        match receiver {
            Receiver::This => (
                match self.class() {
                    Some(class) => Holder::single(Name::from(class)),
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
            Receiver::Static(..) | Receiver::Origins(_) | Receiver::Unknown => (
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
            elements_holder_of(&self.type_of(arg))
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
        true
    }

    fn operand(&self, _: Option<&[Origin]>) -> (Holder, bool) {
        self.operand.clone().unwrap_or((Holder::Unknown, false))
    }

    fn rebindable(&self) -> bool {
        self.checker.shared_scope.closure
    }
}

/// What running the closure written as `id` writes of the variables it
/// captures by reference.
fn captured_effect(arenas: &NodeArenas, id: ExprId) -> Effect {
    let Some(ExprKind::Closure(closure)) = arenas.expr(id).map(|e| &e.kind) else {
        return Effect::nothing();
    };
    let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) else {
        return Effect::nothing();
    };
    Effect::captured(
        id.into_raw().into_u32(),
        arenas
            .closure_uses(&closure.uses)
            .iter()
            .filter(|u| u.by_ref)
            .map(|u| u.name.name.clone()),
    )
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
    /// To what the global of this name may hold in the global scope the
    /// file's top level runs in; when the effect names this global alone
    /// (`named`), to what it holds there now and the functions that bind it
    /// may write.
    TopLevel { global: Name, named: bool },
    /// To anything: a static variable.
    Anything,
    /// To what the place it refers to may hold, by its declaration.
    Reference(Place),
    /// To what it holds and what a closure capturing it may have written.
    Captured,
}

/// The names of the variables an arrow function's body mentions, a closure's
/// `use` list included: what it may take of the code around it.
fn mentioned_variables(arenas: &NodeArenas, body: ExprId) -> Vec<Name> {
    struct Mentions(Vec<Name>);
    impl syntax::visit::arena::Visitor for Mentions {
        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            let Some(e) = arenas.expr(id) else { return };
            match &e.kind {
                ExprKind::Variable(name) => self.0.push(name.clone()),
                ExprKind::Closure(closure) => {
                    if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                        self.0.extend(
                            arenas
                                .closure_uses(&closure.uses)
                                .iter()
                                .map(|u| u.name.name.clone()),
                        );
                    }
                }
                _ => {}
            }
            syntax::visit::arena::walk_expr(self, arenas, id);
        }
    }
    let mut mentions = Mentions(Vec::new());
    syntax::visit::arena::Visitor::visit_expr(&mut mentions, arenas, body);
    mentions.0.sort_unstable();
    mentions.0.dedup();
    mentions.0
}

impl Checker<'_> {
    /// Notes what the closure `id`, written where `locals` is, takes by
    /// value of the variables it names: the object each holds, joined with
    /// what an earlier pass over the code noted.
    pub(crate) fn note_captures(
        &mut self,
        id: ExprId,
        names: impl IntoIterator<Item = Name>,
        locals: &Locals,
    ) {
        let now: rustc_hash::FxHashMap<Name, Holder> = names
            .into_iter()
            .filter_map(|name| {
                let held = holder_of(locals.get_var(&name)?);
                Some((name, held))
            })
            .collect();
        let noted = match self.closure_captures.remove(&id) {
            None => now,
            Some(mut before) => {
                before.retain(|name, held| match now.get(name) {
                    Some(other) => {
                        *held = std::mem::replace(held, Holder::Unknown).join(other.clone());
                        true
                    }
                    None => false,
                });
                before
            }
        };
        self.closure_captures.insert(id, noted);
    }

    /// Notes what the arrow function `id` with body `body`, written where
    /// `locals` is, takes of the variables around it.
    pub(crate) fn note_arrow_captures(
        &mut self,
        arenas: &NodeArenas,
        id: ExprId,
        body: ExprId,
        locals: &Locals,
    ) {
        self.note_captures(id, mentioned_variables(arenas, body), locals);
    }

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
        let before = args
            .iter()
            .filter_map(|arg| {
                let span = self.expr_span(arg.value);
                let ty = self.byref_before.borrow_mut().remove(&span)?;
                Some((span, ty))
            })
            .collect();
        // An array callable on a value the context cannot name is not
        // followed: it may run anything.
        call.callables = call
            .callables
            .iter()
            .filter(|(_, callable)| {
                !matches!(
                    callable,
                    Callable::Pair(
                        PairTarget::Object(
                            Receiver::Static(..) | Receiver::Origins(_) | Receiver::Unknown
                        ),
                        _
                    )
                )
            })
            .cloned()
            .collect();
        if let Some(string_arguments) = self.catalog_string_arguments_at(self.expr_span(e.id))
            && self.forget_catalog_string_arguments_may_change(e, &string_arguments, false, locals)
        {
            return;
        }
        let effect = {
            let ctx = TypedCall {
                checker: self,
                locals,
                callee,
                receiver,
                arenas: Some(arenas),
                args,
                written: Default::default(),
                operand: None,
                before,
            };
            let mut effect = self.registry.call_effect(&call, &ctx);
            effect.join(&ctx.written.borrow());
            effect
        };
        self.forget_effect(&effect, locals);
    }

    /// Applies the ordinary checker transfer for the string-conversion
    /// invocations selected by the catalog graph. Returning false leaves the
    /// call on its generic effect path when the signature or syntax binding
    /// is unavailable.
    pub(crate) fn forget_catalog_string_arguments_may_change(
        &self,
        e: &ExprNode,
        string_arguments: &[u32],
        php_throw_path: bool,
        locals: &mut Locals,
    ) -> bool {
        let Some(signature) = self.registry.lookup_function("strlen") else {
            return false;
        };
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return false;
        };
        let arenas = syntax_owner.arenas();
        let args = match &e.kind {
            ExprKind::Call(_, args) => arenas.args(args.clone()),
            _ => return false,
        };
        let resolved = crate::checker::calls::resolve_call_args(arenas, &args, signature.params());
        for argument in string_arguments {
            if let Some(value) = resolved.get(*argument as usize).and_then(|value| *value) {
                let ty = self.infer_expr(value, locals);
                let is_temporary = self.syntax_arenas().is_some_and(|arenas| {
                    arenas
                        .peeled(value.id)
                        .and_then(|id| arenas.expr(id))
                        .is_some_and(|expression| matches!(expression.kind, ExprKind::New(..)))
                });
                let callback_summary = self.catalog_string_callback_outcomes(&ty, locals);
                let callback_transfer = callback_summary.as_ref().and_then(|summary| {
                    if php_throw_path {
                        let throwing = merge_callback_transfers(
                            summary.throwing.clone(),
                            summary.invalid_return.clone(),
                        );
                        if is_temporary {
                            // A PHP throw can also originate in the
                            // destructor after a normally returning cast.
                            // Keep that callback write state in the join.
                            merge_callback_transfers(throwing, summary.normal.clone())
                        } else {
                            throwing
                        }
                    } else {
                        summary.normal.clone()
                    }
                });
                if let Some(transfer) = callback_transfer {
                    self.forget_operation_may_change_except(
                        &ty,
                        Operation::String,
                        &transfer,
                        locals,
                    );
                } else {
                    self.forget_what_an_operation_on_may_change(value, Operation::String, locals);
                }
                // A direct `new` argument is a temporary owned by this call.
                // PHP releases it after conversion on both normal and PHP-
                // throw paths; apply the existing conservative destructor
                // write summary after the __toString effect.
                if is_temporary {
                    if let Some(summary) = self.catalog_string_destructor_outcomes(&ty, locals) {
                        // On a callback-throw path, normal destructor
                        // completion preserves the callback exception and a
                        // destructor throw replaces it. Join both states;
                        // the semantic evaluator carries the distinct exits.
                        let transfer = if php_throw_path {
                            merge_callback_transfers(summary.normal, summary.throwing)
                        } else {
                            summary.normal
                        };
                        if let Some(transfer) = transfer {
                            self.forget_operation_may_change_except(
                                &ty,
                                Operation::Destroy,
                                &transfer,
                                locals,
                            );
                        }
                    } else {
                        self.forget_what_an_operation_may_change(
                            &ty,
                            false,
                            Operation::Destroy,
                            locals,
                        );
                    }
                }
            } else {
                // `resolve_call_args` deliberately leaves unpacked values
                // unresolved. Join their possible element types before
                // applying the operation. For non-array or otherwise unknown
                // iterables, mixed keeps the effect conservative.
                let values = args
                    .iter()
                    .filter(|argument| argument.spread)
                    .filter_map(|spread| arenas.expr(spread.value))
                    .map(|value| {
                        self.infer_expr(value, locals)
                            .array_value_type()
                            .unwrap_or_else(types::Type::mixed)
                    })
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    self.forget_what_an_operation_may_change(
                        &Type::union_of(values),
                        false,
                        Operation::String,
                        locals,
                    );
                }
            }
        }
        true
    }

    /// Finds a deliberately narrow, source-backed case where a final
    /// `__toString` body consists of simple property writes, pure branches,
    /// literal string returns, and direct throws. Unsupported shapes retain
    /// the ordinary whole-effect invalidation.
    fn catalog_string_callback_outcomes(
        &self,
        ty: &Type,
        locals: &Locals,
    ) -> Option<CatalogCallbackSummary> {
        let classes = ty.object_class_names();
        if classes.len() != 1 {
            return None;
        }
        let class = classes.first()?;
        if !self.registry.class_by_name(class)?.is_final() {
            return None;
        }
        let (owner, _) = self
            .registry
            .lookup_method_with_owner(class, "__toString")?;
        let syntax_owner = self.syntax_tree_owner()?;
        let arenas = syntax_owner.arenas();
        let body = find_method_body(arenas, syntax_owner.statements(), &owner, "__toString")?;
        let strict_types =
            crate::checker::keywords::strict_types_mode(arenas, syntax_owner.statements())?;
        let (throw_writes, normal_writes, invalid_writes, all_writes) =
            callback_writes_by_outcome(arenas, &body, !owner.contains('\\'), strict_types)?;
        let mut branch_reads = Vec::new();
        collect_branch_reads(arenas, &body.0, &mut branch_reads, !owner.contains('\\'))?;
        for path in branch_reads {
            let mut current = owner.clone();
            for (index, property) in path.iter().enumerate() {
                let reflection = self.registry.lookup_property(&current, property.as_str())?;
                if current != owner && reflection.visibility() != syntax::ast::Visibility::Public {
                    return None;
                }
                if reflection.has_get_hook() {
                    return None;
                }
                if !reflection.has_default() {
                    return None;
                }
                if index + 1 < path.len() {
                    let classes = reflection.ty().object_class_names();
                    if classes.len() != 1 || !reflection.ty().is_object().yes() {
                        return None;
                    }
                    current.clone_from(&classes[0]);
                }
            }
        }
        for (path, value) in &all_writes {
            let mut current = owner.clone();
            for (index, property) in path.iter().enumerate() {
                let reflection = self.registry.lookup_property(&current, property.as_str())?;
                if current != owner && reflection.visibility() != syntax::ast::Visibility::Public {
                    return None;
                }
                if index + 1 == path.len() {
                    let assigned = arenas.expr(*value)?;
                    let assigned_ty = self.infer_expr(assigned, locals);
                    if reflection.has_set_hook()
                        || reflection.is_readonly()
                        || reflection.is_virtual()
                        || !self.accepts(reflection.ty_native(), &assigned_ty).yes()
                        || !reflection.ty().is_object().no()
                    {
                        return None;
                    }
                } else {
                    if reflection.has_get_hook()
                        || !reflection.has_default()
                        || !reflection.ty().is_object().yes()
                    {
                        return None;
                    }
                    let classes = reflection.ty().object_class_names();
                    if classes.len() != 1 {
                        return None;
                    }
                    current.clone_from(&classes[0]);
                }
            }
        }
        let transfer = |outcome_writes: &[(Vec<Name>, ExprId)]| -> Option<CatalogThrowTransfer> {
            let reachable_properties: BTreeSet<_> = outcome_writes
                .iter()
                .filter_map(|(path, _)| path.last().cloned())
                .collect();
            let mut reachable_writes = Vec::new();
            for (path, value) in outcome_writes {
                let (class, property) = callback_target_property(&self.registry, &owner, path)?;
                let value = arenas.expr(*value)?;
                reachable_writes.push((class, property, self.infer_expr(value, locals)));
            }
            let mut unreachable_properties = BTreeSet::new();
            for (path, _) in &all_writes {
                let (_, property) = callback_target_property(&self.registry, &owner, path)?;
                if !reachable_properties.contains(&property) {
                    unreachable_properties.insert(property);
                }
            }
            Some(CatalogThrowTransfer {
                unreachable_properties,
                reachable_writes,
            })
        };
        Some(CatalogCallbackSummary {
            throwing: throw_writes.as_deref().map(transfer).flatten(),
            normal: normal_writes.as_deref().map(transfer).flatten(),
            invalid_return: invalid_writes.as_deref().map(transfer).flatten(),
        })
    }

    fn catalog_string_destructor_outcomes(
        &self,
        ty: &Type,
        locals: &Locals,
    ) -> Option<CatalogDestructorSummary> {
        let classes = ty.object_class_names();
        if classes.len() != 1 || !self.registry.class_by_name(&classes[0])?.is_final() {
            return None;
        }
        let class = &classes[0];
        let (owner, _) = self
            .registry
            .lookup_method_with_owner(class, "__destruct")?;
        let syntax_owner = self.syntax_tree_owner()?;
        let arenas = syntax_owner.arenas();
        let body = find_method_body(arenas, syntax_owner.statements(), &owner, "__destruct")?;
        let (throw_writes, normal_writes, all_writes) =
            destructor_writes_by_outcome(arenas, &body, !owner.contains('\\'))?;

        let mut branch_reads = Vec::new();
        collect_branch_reads(arenas, &body.0, &mut branch_reads, !owner.contains('\\'))?;
        for path in branch_reads {
            validate_callback_property_path(&self.registry, &owner, &path)?;
        }
        for (path, value) in &all_writes {
            let (class, property) = callback_target_property(&self.registry, &owner, path)?;
            let reflection = self.registry.lookup_property(&class, property.as_str())?;
            let assigned = arenas.expr(*value)?;
            let assigned_ty = self.infer_expr(assigned, locals);
            if reflection.has_set_hook()
                || reflection.is_readonly()
                || reflection.is_virtual()
                || !self.accepts(reflection.ty_native(), &assigned_ty).yes()
                || !reflection.ty().is_object().no()
            {
                return None;
            }
            validate_callback_property_path(&self.registry, &owner, &path[..path.len() - 1])?;
        }

        let transfer = |writes: &[(Vec<Name>, ExprId)]| -> Option<CatalogThrowTransfer> {
            let reachable_properties = writes
                .iter()
                .filter_map(|(path, _)| path.last().cloned())
                .collect::<BTreeSet<_>>();
            let mut reachable_writes = Vec::new();
            for (path, value) in writes {
                let (class, property) = callback_target_property(&self.registry, &owner, path)?;
                let value = arenas.expr(*value)?;
                reachable_writes.push((class, property, self.infer_expr(value, locals)));
            }
            let unreachable_properties = all_writes
                .iter()
                .filter_map(|(path, _)| path.last().cloned())
                .filter(|property| !reachable_properties.contains(property))
                .collect();
            Some(CatalogThrowTransfer {
                unreachable_properties,
                reachable_writes,
            })
        };
        Some(CatalogDestructorSummary {
            throwing: throw_writes.as_deref().map(transfer).flatten(),
            normal: normal_writes.as_deref().map(transfer).flatten(),
        })
    }

    /// Whether a source-backed, bounded `__toString` body can only end in an
    /// invalid result (and therefore raises TypeError after its body runs).
    pub(crate) fn catalog_string_callback_definitely_invalid(
        &self,
        ty: &Type,
        locals: &Locals,
    ) -> bool {
        let Some(summary) = self.catalog_string_callback_outcomes(ty, locals) else {
            return false;
        };
        summary.normal.is_none() && summary.invalid_return.is_some() && summary.throwing.is_none()
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
        let destroys = matches!(operation, Operation::Destroy);
        if ty.is_void() || (ty.is_object().no() && !(destroys && !ty.is_array().no())) {
            return;
        }
        let effect = self.operation_effect(ty, this, operation, locals);
        self.forget_effect(&effect, locals);
    }

    fn operation_effect(
        &self,
        ty: &Type,
        this: bool,
        operation: Operation,
        locals: &Locals,
    ) -> Effect {
        let destroys = matches!(operation, Operation::Destroy);
        let operand = if destroys {
            elements_holder_of(ty)
        } else {
            holder_of(ty)
        };
        let compute = |operand: Holder, locals: &Locals| {
            let call = CallWrites {
                callee: Callee::Implicit {
                    on: None,
                    operation: operation.clone(),
                },
                arguments: Box::new([]),
                places: Box::new([]),
                objectless: Box::new([]),
                callables: Box::new([]),
                fills: Box::new([]),
            };
            let ctx = TypedCall {
                checker: self,
                locals,
                callee: None,
                receiver: None,
                arenas: self.syntax_arenas(),
                args: &[],
                written: Default::default(),
                operand: Some((operand, this)),
                before: Default::default(),
            };
            self.registry.call_effect(&call, &ctx)
        };
        if effect_memo_enabled() {
            let key = (operation.clone(), operand, this);
            let known = {
                let mut memo = self.operation_effects.borrow_mut();
                memo.in_scope(
                    self.current_class.as_deref(),
                    self.current_file_id,
                    self.shared_scope.closure,
                );
                memo.answers.get(&key).cloned()
            };
            match known {
                Some(effect) => {
                    syntax::stats::count(syntax::stats::Counter::OperationsHit);
                    debug_assert_eq!(effect, compute(key.1.clone(), locals));
                    effect
                }
                None => {
                    syntax::stats::count(syntax::stats::Counter::OperationsMiss);
                    let effect = compute(key.1.clone(), locals);
                    self.operation_effects
                        .borrow_mut()
                        .answers
                        .insert(key, effect.clone());
                    effect
                }
            }
        } else {
            compute(operand, locals)
        }
    }

    fn forget_operation_may_change_except(
        &self,
        ty: &Type,
        operation: Operation,
        transfer: &CatalogThrowTransfer,
        locals: &mut Locals,
    ) {
        if !locals.has_state_to_forget() && self.shared_scope.binds_nothing() {
            return;
        }
        if ty.is_object().no() || ty.is_void() {
            return;
        }
        let effect = self
            .operation_effect(ty, false, operation, locals)
            .without_properties(&transfer.unreachable_properties);
        let before = locals.clone();
        self.forget_effect(&effect, locals);
        let keys: Vec<_> = before.synthetic_keys().copied().collect();
        for (class, property, assigned) in &transfer.reachable_writes {
            for key in &keys {
                let KeyData::Prop { base, prop } = before.data(*key) else {
                    continue;
                };
                if prop != *property
                    || !self.may_hold(&self.key_type(base, &before), Some(class.as_str()))
                {
                    continue;
                }
                let Some(previous) = before
                    .get(*key)
                    .cloned()
                    .or_else(|| self.key_type(*key, &before))
                else {
                    continue;
                };
                let joined = self.relation().union_of(vec![previous, assigned.clone()]);
                locals.insert(*key, joined);
            }
        }
    }

    /// After the value of the expression `e`, of type `ty`, is dropped where
    /// it was made, what destroying it may have written is stale; a value
    /// that is held by something else is not dropped.
    pub(crate) fn forget_what_a_temporary_destroys(
        &self,
        e: &ExprNode,
        ty: &Type,
        locals: &mut Locals,
    ) {
        if matches!(
            e.kind,
            ExprKind::New(..)
                | ExprKind::Clone(..)
                | ExprKind::ArrayLiteral(..)
                | ExprKind::Call(..)
                | ExprKind::MethodCall(..)
                | ExprKind::DynamicMethodCall(..)
                | ExprKind::StaticCall(..)
                | ExprKind::DynamicStaticCall(..)
        ) {
            self.forget_what_an_operation_may_change(ty, false, Operation::Destroy, locals);
        }
    }

    /// After the place `target` is overwritten, what destroying the value it
    /// held may have written is stale; a variable that holds nothing yet
    /// destroys nothing.
    pub(crate) fn forget_what_overwriting_may_change(
        &self,
        target: &ExprNode,
        locals: &mut Locals,
    ) {
        if matches!(&target.kind, ExprKind::Variable(name) if !locals.contains_var(name)) {
            return;
        }
        self.forget_what_an_operation_on_may_change(target, Operation::Destroy, locals);
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

    /// After the include `at`, what the top level of the file it names, or
    /// any code when it names none, may have written is stale.
    pub(crate) fn forget_what_an_include_may_change(&self, at: ExprId, locals: &mut Locals) {
        let call = CallWrites {
            callee: Callee::Include(u32::from(at.into_raw())),
            arguments: Box::new([]),
            places: Box::new([]),
            objectless: Box::new([]),
            callables: Box::new([]),
            fills: Box::new([]),
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
                operand: None,
                before: Default::default(),
            };
            let mut effect = self.registry.call_effect(&call, &ctx);
            effect.join(&ctx.written.borrow());
            effect
        };
        self.forget_effect(&effect, locals);
    }

    /// Forgets the variables the include `at` may write: the file it
    /// includes assigns them in this scope.
    pub(crate) fn forget_variables_an_include_writes(&self, at: ExprId, locals: &mut Locals) {
        let writes = self
            .registry
            .include_writes(self.current_file_id, u32::from(at.into_raw()));
        let written: Vec<Name> = locals
            .variables()
            .into_iter()
            .map(|(name, _)| name)
            .filter(|name| **name != *"this" && writes.may_write(name))
            .collect();
        for name in written {
            purge_stale_narrowing_for_var(locals, &name);
            locals.insert_var(&name, Type::mixed());
            locals.clear_native_var(&name);
        }
    }

    /// Forgets what `effect` may have written.
    pub(crate) fn forget_effect(&self, effect: &Effect, locals: &mut Locals) {
        let scope = Rc::clone(&self.shared_scope);
        if effect.is_nothing() {
            return;
        }
        let anything = effect.is_anything();
        let storages: Vec<&Storage> = effect.storages().into_iter().flatten().collect();
        let mut stale: Vec<LocalKey> = Vec::new();
        for &key in locals.synthetic_keys() {
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
        for (name, shared) in self.shared_variables_written(
            &storages,
            anything,
            effect.is_arbitrary(),
            &scope,
            locals,
        ) {
            let ty = match shared {
                Shared::Global(global) => self
                    .registry
                    .global_in_scope_of(None, global.as_str())
                    .unwrap_or_else(Type::mixed),
                Shared::TopLevel { global, named } => {
                    let written = named
                        .then(|| {
                            self.registry
                                .global_written_by_functions(global.as_str(), self.current_file_id)
                        })
                        .flatten();
                    match (locals.get_var(&name), written) {
                        (Some(held), Some(written)) => {
                            self.relation().union_of(vec![held.clone(), written])
                        }
                        _ => self
                            .registry
                            .global_in_scope_of(self.current_file_id, global.as_str())
                            .unwrap_or_else(Type::mixed),
                    }
                }
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
            && scope.unescaped_at(&name, self.stmt_at.get())
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
        if self.stable_place(base, prop, locals) {
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
            Storage::Static { .. }
            | Storage::Global(_)
            | Storage::StaticVar { .. }
            | Storage::Captured { .. }
            | Storage::Unknown => false,
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

    /// Whether `prop` of the value at `base`, or an element of the property
    /// the key `base` is (when `prop` is `None`), is a property no code of
    /// the loaded program writes once the object is made.
    fn stable_place(&self, base: LocalKey, prop: Option<&Name>, locals: &Locals) -> bool {
        // A constructor runs on the object while the scope knows it: a call
        // to the parent's constructor writes what the facts are about.
        if self.current_method.as_deref().is_some_and(|method| {
            method.eq_ignore_ascii_case("__construct") || method.eq_ignore_ascii_case("__clone")
        }) {
            return false;
        }
        let (base, prop) = match prop {
            Some(prop) => (base, prop.clone()),
            None => {
                let mut key = base;
                loop {
                    match locals.data(key) {
                        KeyData::Index { base, .. } | KeyData::VarIndex { base, .. } => key = base,
                        KeyData::Prop { base, prop } => break (base, prop),
                        _ => return false,
                    }
                }
            }
        };
        let Some(ty) = self.key_type(base, locals) else {
            return false;
        };
        if !ty.is_object().yes() {
            return false;
        }
        let classes = ty.object_class_names();
        let assume = self.settings.rules.assume_no_external_property_writers;
        !classes.is_empty()
            && classes
                .iter()
                .all(|class| self.registry.property_is_stable(class, &prop, assume))
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
                    .chain(
                        self.registry
                            .subclasses_shared(own)
                            .iter()
                            .map(|c| c.to_string()),
                    )
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
        arbitrary: bool,
        scope: &SharedScope,
        locals: &Locals,
    ) -> Vec<(Name, Shared)> {
        let mut out: Vec<(Name, Shared)> = Vec::new();
        // Code of the loaded program writes a global only where a function
        // body writes it or binds it.
        let global_written = |name: &Name, top_level_of: Option<FileId>| {
            arbitrary
                || (anything && self.registry.code_may_write_global(name, top_level_of))
                || storages.iter().any(|storage| match storage {
                    Storage::Global(None) => true,
                    Storage::Global(Some(global)) => global == name,
                    _ => false,
                })
        };
        for name in &scope.globals {
            if global_written(name, None) {
                out.push((name.clone(), Shared::Global(name.clone())));
            }
        }
        // Every variable is a global at a file's top level, and may be one
        // after a `global $$n`; the superglobals are everywhere.
        let every_variable = scope.top_level || scope.any_global;
        let mut visit = |name: &Name| {
            let global = scope.top_level
                || scope.any_global
                || (is_superglobal(name) && **name != *"GLOBALS");
            let top_level_of = scope.top_level.then_some(self.current_file_id).flatten();
            if global && global_written(name, top_level_of) {
                let shared = if scope.top_level {
                    Shared::TopLevel {
                        global: name.clone(),
                        named: !anything
                            && !is_superglobal(name)
                            && !storages
                                .iter()
                                .any(|storage| matches!(storage, Storage::Global(None)))
                            && storages.iter().any(|storage| {
                                matches!(storage, Storage::Global(Some(global)) if global == name)
                            }),
                    }
                } else {
                    Shared::Global(name.clone())
                };
                out.push((name.clone(), shared));
            }
        };
        if every_variable {
            locals.each_variable(|name, _| visit(name));
        } else {
            let found = locals.variables_named(&SUPERGLOBALS_BUT_GLOBALS);
            debug_assert!(
                found.iter().map(|(name, _)| name).eq(locals
                    .variables()
                    .iter()
                    .map(|(name, _)| name)
                    .filter(|name| is_superglobal(name) && ***name != *"GLOBALS")),
            );
            for (name, _) in &found {
                visit(name);
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
        let at = self.stmt_at.get();
        for (name, capture) in &scope.captured {
            let run = capture.closures.iter().any(|&closure| {
                storages.iter().any(|storage| {
                    matches!(storage, Storage::Captured { closure: found, var }
                        if *found == closure && var == name)
                })
            });
            // Unknown code runs a closure it may hold, or the scope's own.
            let unknown = anything
                && (capture.own
                    || capture
                        .closures
                        .iter()
                        .any(|&closure| scope.closure_left_at(closure, at)));
            if run || unknown {
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

#[derive(Clone)]
struct CatalogThrowTransfer {
    unreachable_properties: BTreeSet<Name>,
    reachable_writes: Vec<(Name, Name, Type)>,
}

struct CatalogCallbackSummary {
    throwing: Option<CatalogThrowTransfer>,
    normal: Option<CatalogThrowTransfer>,
    invalid_return: Option<CatalogThrowTransfer>,
}

struct CatalogDestructorSummary {
    throwing: Option<CatalogThrowTransfer>,
    normal: Option<CatalogThrowTransfer>,
}

fn merge_callback_transfers(
    left: Option<CatalogThrowTransfer>,
    right: Option<CatalogThrowTransfer>,
) -> Option<CatalogThrowTransfer> {
    match (left, right) {
        (None, None) => None,
        (Some(transfer), None) | (None, Some(transfer)) => Some(transfer),
        (Some(mut left), Some(right)) => {
            left.unreachable_properties
                .retain(|property| right.unreachable_properties.contains(property));
            left.reachable_writes.extend(right.reachable_writes);
            Some(left)
        }
    }
}

fn callback_target_property(
    registry: &crate::registry::Registry,
    owner: &Name,
    path: &[Name],
) -> Option<(Name, Name)> {
    let mut current = owner.clone();
    for property in &path[..path.len().checked_sub(1)?] {
        let reflection = registry.lookup_property(&current, property.as_str())?;
        let classes = reflection.ty().object_class_names();
        if classes.len() != 1 {
            return None;
        }
        current.clone_from(&classes[0]);
    }
    Some((current, path.last()?.clone()))
}

fn validate_callback_property_path(
    registry: &crate::registry::Registry,
    owner: &Name,
    path: &[Name],
) -> Option<()> {
    let mut current = owner.clone();
    for (index, property) in path.iter().enumerate() {
        let reflection = registry.lookup_property(&current, property.as_str())?;
        if current != *owner && reflection.visibility() != syntax::ast::Visibility::Public {
            return None;
        }
        if reflection.has_get_hook() || !reflection.has_default() {
            return None;
        }
        if index + 1 < path.len() {
            let classes = reflection.ty().object_class_names();
            if classes.len() != 1 || !reflection.ty().is_object().yes() {
                return None;
            }
            current.clone_from(&classes[0]);
        }
    }
    Some(())
}

fn find_method_body(
    arenas: &NodeArenas,
    statements: &[StmtId],
    owner: &str,
    method: &str,
) -> Option<(Vec<StmtId>, bool)> {
    struct Find<'a> {
        owner: &'a str,
        method: &'a str,
        found: Option<(Vec<StmtId>, bool)>,
    }

    impl Visitor for Find<'_> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            if let StmtKind::ClassDecl(decl_range) = &stmt.kind {
                let decl = arenas.class_decl(decl_range.clone());
                if decl.namespaced_name.eq_ignore_ascii_case(self.owner)
                    && let Some(class) = arenas.class(decl.def)
                {
                    for method in arenas.methods(class.methods.clone()) {
                        let function = &method.function;
                        if function.name.name.eq_ignore_ascii_case(self.method)
                            && let Some(body) = &function.body
                        {
                            self.found = Some((
                                arenas.stmt_ids(&body.stmts).to_vec(),
                                body.goto_labels.is_empty(),
                            ));
                            return;
                        }
                    }
                }
                return;
            }
            arena::walk_stmt(self, arenas, id);
        }
    }

    let mut find = Find {
        owner,
        method,
        found: None,
    };
    arena::walk_stmts(&mut find, arenas, statements);
    find.found
}

/// Interpret a small source subset to collect writes on paths which end in a
/// direct throw, valid literal string return, or invalid literal scalar return.
/// Branches are forked, so a write in one outcome does not leak into the other.
/// Unsupported live statements and implicit returns fail closed.
fn callback_writes_by_outcome(
    arenas: &NodeArenas,
    (statements, has_no_goto_labels): &(Vec<StmtId>, bool),
    allow_unqualified_runtime_exception: bool,
    strict_types: bool,
) -> Option<(
    Option<Vec<(Vec<Name>, ExprId)>>,
    Option<Vec<(Vec<Name>, ExprId)>>,
    Option<Vec<(Vec<Name>, ExprId)>>,
    Vec<(Vec<Name>, ExprId)>,
)> {
    if !has_no_goto_labels {
        return None;
    }
    let mut all_writes = Vec::new();
    collect_literal_assignments(
        arenas,
        statements,
        &mut all_writes,
        &mut Vec::new(),
        allow_unqualified_runtime_exception,
    )?;
    let mut states = vec![CallbackPath {
        writes: Vec::new(),
        outcome: CallbackOutcome::Live,
    }];
    execute_callback_statements(arenas, statements, &mut states, strict_types, false)?;
    if states.iter().any(|state| {
        !matches!(
            state.outcome,
            CallbackOutcome::Throw | CallbackOutcome::NormalReturn | CallbackOutcome::InvalidReturn
        )
    }) {
        return None;
    }
    let throw_paths = states
        .iter()
        .filter(|state| state.outcome == CallbackOutcome::Throw)
        .collect::<Vec<_>>();
    let normal_paths = states
        .iter()
        .filter(|state| state.outcome == CallbackOutcome::NormalReturn)
        .collect::<Vec<_>>();
    let invalid_paths = states
        .iter()
        .filter(|state| state.outcome == CallbackOutcome::InvalidReturn)
        .collect::<Vec<_>>();
    if throw_paths.is_empty() && normal_paths.is_empty() && invalid_paths.is_empty() {
        return None;
    }
    let throw_writes = (!throw_paths.is_empty()).then(|| {
        throw_paths
            .iter()
            .flat_map(|state| state.writes.iter().cloned())
            .collect()
    });
    let normal_writes = (!normal_paths.is_empty()).then(|| {
        normal_paths
            .iter()
            .flat_map(|state| state.writes.iter().cloned())
            .collect()
    });
    let invalid_writes = (!invalid_paths.is_empty()).then(|| {
        invalid_paths
            .iter()
            .flat_map(|state| state.writes.iter().cloned())
            .collect()
    });
    Some((throw_writes, normal_writes, invalid_writes, all_writes))
}

fn destructor_writes_by_outcome(
    arenas: &NodeArenas,
    (statements, has_no_goto_labels): &(Vec<StmtId>, bool),
    allow_unqualified_runtime_exception: bool,
) -> Option<(
    Option<Vec<(Vec<Name>, ExprId)>>,
    Option<Vec<(Vec<Name>, ExprId)>>,
    Vec<(Vec<Name>, ExprId)>,
)> {
    if !has_no_goto_labels {
        return None;
    }
    let mut all_writes = Vec::new();
    collect_literal_assignments(
        arenas,
        statements,
        &mut all_writes,
        &mut Vec::new(),
        allow_unqualified_runtime_exception,
    )?;
    let mut states = vec![CallbackPath {
        writes: Vec::new(),
        outcome: CallbackOutcome::Live,
    }];
    execute_callback_statements(arenas, statements, &mut states, false, true)?;
    for state in &mut states {
        if state.outcome == CallbackOutcome::Live {
            state.outcome = CallbackOutcome::NormalReturn;
        }
    }
    if states.iter().any(|state| {
        !matches!(
            state.outcome,
            CallbackOutcome::Throw | CallbackOutcome::NormalReturn
        )
    }) {
        return None;
    }
    let throw_paths = states
        .iter()
        .filter(|state| state.outcome == CallbackOutcome::Throw)
        .collect::<Vec<_>>();
    let normal_paths = states
        .iter()
        .filter(|state| state.outcome == CallbackOutcome::NormalReturn)
        .collect::<Vec<_>>();
    let throw_writes = (!throw_paths.is_empty()).then(|| {
        throw_paths
            .iter()
            .flat_map(|state| state.writes.iter().cloned())
            .collect()
    });
    let normal_writes = (!normal_paths.is_empty()).then(|| {
        normal_paths
            .iter()
            .flat_map(|state| state.writes.iter().cloned())
            .collect()
    });
    Some((throw_writes, normal_writes, all_writes))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallbackOutcome {
    Live,
    Throw,
    NormalReturn,
    InvalidReturn,
}

#[derive(Clone)]
struct CallbackPath {
    writes: Vec<(Vec<Name>, ExprId)>,
    outcome: CallbackOutcome,
}

fn collect_literal_assignments(
    arenas: &NodeArenas,
    statements: &[StmtId],
    out: &mut Vec<(Vec<Name>, ExprId)>,
    reads: &mut Vec<Vec<Name>>,
    allow_unqualified_runtime_exception: bool,
) -> Option<()> {
    for id in statements {
        let stmt = arenas.stmt(*id)?;
        match &stmt.kind {
            StmtKind::Expression(expression) => {
                out.push(simple_literal_property_assignment(arenas, *expression)?);
            }
            StmtKind::Block(body) => {
                collect_literal_assignments(
                    arenas,
                    arenas.stmt_ids(body),
                    out,
                    reads,
                    allow_unqualified_runtime_exception,
                )?;
            }
            StmtKind::If(branch) => {
                simple_branch_condition(arenas, branch.cond, reads)?;
                collect_literal_assignments(
                    arenas,
                    &[branch.then_branch],
                    out,
                    reads,
                    allow_unqualified_runtime_exception,
                )?;
                for elseif in arenas.elseifs(branch.elseifs.clone()) {
                    simple_branch_condition(arenas, elseif.cond, reads)?;
                    collect_literal_assignments(
                        arenas,
                        &[elseif.body],
                        out,
                        reads,
                        allow_unqualified_runtime_exception,
                    )?;
                }
                if let Some(otherwise) = branch.else_branch {
                    collect_literal_assignments(
                        arenas,
                        &[otherwise],
                        out,
                        reads,
                        allow_unqualified_runtime_exception,
                    )?;
                }
            }
            StmtKind::Throw(value) => {
                if !is_noarg_runtime_exception(arenas, *value, allow_unqualified_runtime_exception)
                {
                    return None;
                }
            }
            StmtKind::Return(None) => {}
            StmtKind::Return(Some(value)) => {
                // Returning a value may invoke user code while evaluating it;
                // accept only side-effect-free values we can classify.
                if callback_return_is_definitely_invalid(arenas, *value, false).is_none() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(())
}

fn collect_branch_reads(
    arenas: &NodeArenas,
    statements: &[StmtId],
    reads: &mut Vec<Vec<Name>>,
    allow_unqualified_runtime_exception: bool,
) -> Option<()> {
    let mut ignored_writes = Vec::new();
    collect_literal_assignments(
        arenas,
        statements,
        &mut ignored_writes,
        reads,
        allow_unqualified_runtime_exception,
    )
}

fn simple_branch_condition(
    arenas: &NodeArenas,
    id: ExprId,
    reads: &mut Vec<Vec<Name>>,
) -> Option<()> {
    let expression = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    match &expression.kind {
        ExprKind::Variable(_) | ExprKind::Literal(_) => Some(()),
        ExprKind::PropertyAccess(..) => {
            reads.push(simple_this_property_path(arenas, id)?);
            Some(())
        }
        _ => None,
    }
}

/// Weak scalar returns are converted to strings by PHP's magic-method
/// boundary. Null and an empty array fail that boundary. Other expressions
/// need ordinary conservative handling because evaluating them may run code.
fn callback_return_is_definitely_invalid(
    arenas: &NodeArenas,
    id: ExprId,
    strict_types: bool,
) -> Option<bool> {
    let expression = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    match &expression.kind {
        ExprKind::Literal(Literal::Null) => Some(true),
        ExprKind::Literal(Literal::String(_)) => Some(false),
        ExprKind::Literal(_) => Some(strict_types),
        ExprKind::ArrayLiteral(items) if arenas.array_items(items.clone()).is_empty() => Some(true),
        _ => None,
    }
}

fn simple_this_property_path(arenas: &NodeArenas, id: ExprId) -> Option<Vec<Name>> {
    fn visit(arenas: &NodeArenas, id: ExprId, path: &mut Vec<Name>) -> Option<()> {
        let expression = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        match &expression.kind {
            ExprKind::Variable(name) if **name == *"this" => Some(()),
            ExprKind::PropertyAccess(base, property, _) => {
                visit(arenas, *base, path)?;
                path.push(property.name.clone());
                Some(())
            }
            _ => None,
        }
    }
    let mut path = Vec::new();
    visit(arenas, id, &mut path)?;
    (!path.is_empty()).then_some(path)
}

fn is_noarg_runtime_exception(arenas: &NodeArenas, id: ExprId, allow_unqualified: bool) -> bool {
    let Some(expression) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return false;
    };
    let ExprKind::New(callee, args) = &expression.kind else {
        return false;
    };
    if !arenas.args(args.clone()).is_empty() {
        return false;
    }
    let Some(ExprKind::Name(name)) = arenas.expr(*callee).map(|callee| &callee.kind) else {
        return false;
    };
    let Some(name) = arenas.qualified_name(name) else {
        return false;
    };
    matches!(&name.resolved, ResolvedName::Exact(name) if name.eq_ignore_ascii_case("RuntimeException"))
        || (allow_unqualified && name.written.eq_ignore_ascii_case("RuntimeException"))
}

fn execute_callback_statements(
    arenas: &NodeArenas,
    statements: &[StmtId],
    states: &mut Vec<CallbackPath>,
    strict_types: bool,
    destructor: bool,
) -> Option<()> {
    for id in statements {
        let stmt = arenas.stmt(*id)?;
        let mut next = Vec::new();
        for mut state in std::mem::take(states) {
            if state.outcome != CallbackOutcome::Live {
                next.push(state);
                continue;
            }
            match &stmt.kind {
                StmtKind::Expression(expression) => {
                    state
                        .writes
                        .push(simple_literal_property_assignment(arenas, *expression)?);
                    next.push(state);
                }
                StmtKind::Throw(_) => {
                    state.outcome = CallbackOutcome::Throw;
                    next.push(state);
                }
                StmtKind::Return(Some(value)) => {
                    if destructor {
                        return None;
                    }
                    state.outcome =
                        if callback_return_is_definitely_invalid(arenas, *value, strict_types)? {
                            CallbackOutcome::InvalidReturn
                        } else {
                            CallbackOutcome::NormalReturn
                        };
                    next.push(state);
                }
                StmtKind::Return(None) => {
                    state.outcome = if destructor {
                        CallbackOutcome::NormalReturn
                    } else {
                        CallbackOutcome::InvalidReturn
                    };
                    next.push(state);
                }
                StmtKind::Block(body) => {
                    let mut nested = vec![state];
                    execute_callback_statements(
                        arenas,
                        arenas.stmt_ids(body),
                        &mut nested,
                        strict_types,
                        destructor,
                    )?;
                    next.extend(nested);
                }
                StmtKind::If(branch) => {
                    simple_branch_condition(arenas, branch.cond, &mut Vec::new())?;
                    let mut branch_ids = vec![branch.then_branch];
                    branch_ids.extend(
                        arenas
                            .elseifs(branch.elseifs.clone())
                            .iter()
                            .map(|branch| branch.body),
                    );
                    if let Some(otherwise) = branch.else_branch {
                        branch_ids.push(otherwise);
                    } else {
                        next.push(state.clone());
                    }
                    for branch_id in branch_ids {
                        let mut branch_state = vec![state.clone()];
                        execute_callback_statements(
                            arenas,
                            &[branch_id],
                            &mut branch_state,
                            strict_types,
                            destructor,
                        )?;
                        next.extend(branch_state);
                    }
                }
                _ => return None,
            }
        }
        if next.len() > 64 {
            return None;
        }
        *states = next;
    }
    Some(())
}

fn simple_literal_property_assignment(
    arenas: &NodeArenas,
    id: ExprId,
) -> Option<(Vec<Name>, ExprId)> {
    fn property_path(arenas: &NodeArenas, id: ExprId, out: &mut Vec<Name>) -> Option<()> {
        let expression = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
        match &expression.kind {
            ExprKind::Variable(name) if **name == *"this" => Some(()),
            ExprKind::PropertyAccess(base, property, _) => {
                property_path(arenas, *base, out)?;
                out.push(property.name.clone());
                Some(())
            }
            _ => None,
        }
    }

    let expression = arenas.peeled(id).and_then(|id| arenas.expr(id))?;
    let ExprKind::Assign(target, value) = &expression.kind else {
        return None;
    };
    let value_id = arenas.peeled(*value)?;
    let value = arenas.expr(value_id)?;
    if !matches!(value.kind, ExprKind::Literal(_)) {
        return None;
    }
    let mut path = Vec::new();
    property_path(arenas, *target, &mut path)?;
    (path.len() >= 2).then_some((path, value_id))
}
