//! Real PHPStan's `Rules\DeadCode\NoopRule`: a bare expression statement
//! whose result is discarded and which does nothing else.
//!
//! The gate is "no side effects": `expr_side_effects` scans the whole
//! statement, and `Checker::expr_has_hidden_effects` adds the effects
//! only types reveal - a property read through `__get` or a `get` hook,
//! an offset read through `ArrayAccess::offsetGet`, and an object
//! stringified through `__toString`. Anything not provably free of
//! effects is treated as having one.
//!
//! `and`/`or`/`xor` are reported even when the statement assigns a local
//! variable (`$x = true and $y;` discards `$y`), with a precedence tip;
//! `&&`/`||`, the ternary, and every other expression (`expr.resultUnused`,
//! naming the expression) only when the statement assigns nothing.
//! Calls and `new` are left to `check_pure_call_without_effect`.

use crate::checker::{Checker, Locals};
use crate::registry::{FunctionReflection, collect_impure_points};
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use syntax::ast::{BinOp, CastKind, ExprKind, ExprNode, FunctionDecl, UnaryOp};
use syntax::visit::{Visitor, any_expr_in_scope, walk_expr_in_scope};
use types::Type;

const PRECEDENCE_TIP: &str =
    "This operator has unexpected precedence, try disambiguating the logic with parentheses ().";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Effects {
    None,
    LocalAssignOnly,
    Some,
}

fn is_local_assign_target(target: &ExprNode) -> bool {
    syntax::stack::guard(|| match &target.kind {
        ExprKind::Variable(name) => name.as_ref() != "this",
        ExprKind::Destructure(items, _) => items.iter().all(|item| {
            item.key
                .as_ref()
                .is_none_or(|k| expr_side_effects(k) == Effects::None)
                && item.target.as_ref().is_none_or(is_local_assign_target)
        }),
        _ => false,
    })
}

/// The strongest effect anywhere in `e`'s subtree. A nested closure or
/// anonymous class's body doesn't run by being constructed, so it
/// doesn't count.
fn expr_side_effects(e: &ExprNode) -> Effects {
    struct Strongest(Effects);
    impl<'ast> Visitor<'ast> for Strongest {
        fn visit_expr(&mut self, e: &'ast ExprNode) {
            if self.0 == Effects::Some {
                return;
            }
            self.0 = std::cmp::max(self.0, own_side_effects(e));
            walk_expr_in_scope(self, e);
        }
    }
    let mut strongest = Strongest(Effects::None);
    strongest.visit_expr(e);
    strongest.0
}

/// `e`'s own effects, not its operands'.
fn own_side_effects(e: &ExprNode) -> Effects {
    match &e.kind {
        ExprKind::Assign(target, _) | ExprKind::AssignRef(target, _)
            if is_local_assign_target(target) =>
        {
            Effects::LocalAssignOnly
        }
        ExprKind::CompoundAssign(_, target, _) if matches!(target.kind, ExprKind::Variable(_)) => {
            Effects::LocalAssignOnly
        }
        ExprKind::Unary(
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
            target,
        ) if matches!(target.kind, ExprKind::Variable(_)) => Effects::LocalAssignOnly,
        ExprKind::Call(..)
        | ExprKind::MethodCall(..)
        | ExprKind::StaticCall(..)
        | ExprKind::DynamicMethodCall(..)
        | ExprKind::DynamicStaticCall(..)
        | ExprKind::New(..)
        | ExprKind::NewAnonymousClass(..)
        | ExprKind::Assign(..)
        | ExprKind::AssignRef(..)
        | ExprKind::CompoundAssign(..)
        | ExprKind::Throw(..)
        | ExprKind::Clone(..)
        | ExprKind::Yield(..)
        | ExprKind::YieldFrom(..)
        | ExprKind::ShellExec(..)
        | ExprKind::Print(_)
        | ExprKind::Exit(..)
        | ExprKind::Eval(_)
        | ExprKind::Include(..)
        | ExprKind::DynamicVariable(..)
        | ExprKind::Unary(
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
            _,
        ) => Effects::Some,
        _ => Effects::None,
    }
}

impl<'a> Checker<'a> {
    /// Effects that depend on the operands' types: `__get`/`get` hooks on
    /// a property read, `offsetGet` on an object offset read, and
    /// `__toString` on a stringified object.
    fn expr_has_hidden_effects(&self, e: &ExprNode, locals: &Locals) -> bool {
        any_expr_in_scope(e, |e| self.own_hidden_effects(e, locals))
    }

    /// `expr_has_hidden_effects` for `e` itself, not its operands.
    fn own_hidden_effects(&self, e: &ExprNode, locals: &Locals) -> bool {
        match &e.kind {
            ExprKind::PropertyAccess(object, name, _) => {
                !self.property_read_is_plain(&self.infer_expr(object, locals), name)
            }
            ExprKind::DynamicPropertyAccess(..) => true,
            ExprKind::Index(base, _) => !self.offset_read_is_plain(&self.infer_expr(base, locals)),
            ExprKind::Cast(cast, operand) if cast.kind() == CastKind::String => {
                self.may_be_stringable_object(&self.infer_expr(operand, locals))
            }
            ExprKind::Binary(BinOp::Concat, l, r) => [l, r]
                .iter()
                .any(|side| self.may_be_stringable_object(&self.infer_expr(side, locals))),
            ExprKind::Interpolated(parts) => parts
                .iter()
                .any(|part| self.may_be_stringable_object(&self.infer_expr(part, locals))),
            _ => false,
        }
    }

    /// Whether reading property `name` of `receiver` runs no code: a
    /// declared property without a get hook on a class without `__get`,
    /// or an enum case's; `null` may join a union.
    fn property_read_is_plain(&self, receiver: &Type, name: &str) -> bool {
        !receiver.is_null().yes()
            && receiver.union_members().iter().all(|m| {
                m.is_null().yes()
                    || m.is_enum_case()
                    || m.as_object().is_some_and(|(class, _)| {
                        if self.registry.lookup_method(class, "__get").is_some() {
                            return false;
                        }
                        match self.registry.lookup_property(class, name) {
                            Some(prop) => !prop.has_get_hook(),
                            None => self.registry.lookup_class(class).is_some(),
                        }
                    })
            })
    }

    /// Whether reading an offset of `base` runs no code: no member is an
    /// object, which may implement `ArrayAccess`.
    fn offset_read_is_plain(&self, base: &Type) -> bool {
        base.union_members()
            .iter()
            .all(|m| m.is_array().yes() || m.is_scalar().yes() || m.is_null().yes())
    }

    fn may_be_stringable_object(&self, ty: &Type) -> bool {
        ty.union_members().iter().any(|m| m.is_object().yes())
    }

    /// Called from `StmtKind::Expression` after `e` itself was checked.
    pub(crate) fn check_noop_statement(&mut self, e: &ExprNode, locals: &Locals) {
        let e = e.peeled();
        let effects = expr_side_effects(e);
        if effects == Effects::Some || self.expr_has_hidden_effects(e, locals) {
            return;
        }
        let word_operator = match &e.kind {
            ExprKind::Binary(BinOp::LogicalXor, ..) => Some(("xor", "logicalXor")),
            ExprKind::Binary(BinOp::LogicalAnd, ..) => Some(("and", "logicalAnd")),
            ExprKind::Binary(BinOp::LogicalOr, ..) => Some(("or", "logicalOr")),
            _ => None,
        };
        if let Some((sigil, identifier)) = word_operator {
            self.report_builder(
                DiagnosticBuilder::new(
                    format!("Unused result of \"{sigil}\" operator."),
                    e.span,
                    4,
                    format!("{identifier}.resultUnused"),
                )
                .tip(PRECEDENCE_TIP),
            );
            return;
        }
        if effects != Effects::None {
            return;
        }
        let (message, identifier) = match &e.kind {
            ExprKind::Binary(BinOp::And, ..) => (
                "Unused result of \"&&\" operator.".to_string(),
                "booleanAnd.resultUnused",
            ),
            ExprKind::Binary(BinOp::Or, ..) => (
                "Unused result of \"||\" operator.".to_string(),
                "booleanOr.resultUnused",
            ),
            ExprKind::Ternary(..) => (
                "Unused result of ternary operator.".to_string(),
                "ternary.resultUnused",
            ),
            ExprKind::Closure { .. } => return,
            _ => (
                format!(
                    "Expression \"{}\" on a separate line does not do anything.",
                    syntax::printer::print_expr_first_line(e)
                ),
                "expr.resultUnused",
            ),
        };
        self.report(message, e.span, 4, identifier);
    }

    /// The transitive-purity payoff: a bare, statement-level
    /// call whose target `registry::purity::Registry::
    /// resolve_transitive_purity` proved effect-free is dead code, the
    /// exact same family as `check_noop_statement` above (a discarded
    /// call-shaped expression instead of a logical/ternary one) - real
    /// PHPStan's own four `CallTo*StatementWithoutImpurePointsRule`s,
    /// unified into one check here since only the message/identifier
    /// differ per call shape, all four transcribed directly from
    /// `vendor/phpstan-src`, not guessed. Resolution mirrors whatever
    /// this project's own ordinary call-checking already does for each
    /// shape (`infer_expr`/`find_method_owner`/`resolve_static_class_name`),
    /// since real type inference is available here, unlike
    /// `registry::purity`'s own registry-build-time classification pass;
    /// this reads the registry's ID-keyed verdict for whatever declaration
    /// the call actually, concretely resolves to, without rebuilding a
    /// `PurityDependency` key from the call body.
    pub(crate) fn check_pure_call_without_effect(&mut self, e: &ExprNode, locals: &Locals) {
        let (message, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                let Some((resolved, _sig)) = self.registry.resolve_function(name) else {
                    return;
                };
                if !self.registry.function_is_pure(resolved) {
                    return;
                }
                let declared = self
                    .registry
                    .lookup_function_with_name(resolved)
                    .map_or(resolved, |(declared, _)| declared);
                (
                    format!("Call to function {declared}() on a separate line has no effect."),
                    "function.resultUnused",
                )
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                // The callee and receiver the call's own check resolved,
                // when it did.
                let resolved = self
                    .statement_method_call
                    .as_ref()
                    .filter(|call| call.id == e.id)
                    .map(|call| (call.owner.clone(), call.receiver.clone()))
                    .and_then(|(owner, recv_ty)| {
                        let (_, sig) = self.lookup_method_with_owner(&owner, method)?;
                        Some((owner, sig, recv_ty))
                    });
                let Some((owner, sig, recv_ty)) = resolved.or_else(|| {
                    let recv_ty = self.infer_expr(recv, locals);
                    let (owner, sig) = self.find_method_owner(&recv_ty, method)?;
                    Some((owner, sig, recv_ty))
                }) else {
                    return;
                };
                if !self.registry.method_is_pure(&owner, method) {
                    return;
                }
                // Real PHPStan's own `PossiblyPureMethodCallCollector`
                // re-applies the identical overridability gate
                // `registry::purity`'s own classification pass already
                // used to decide the registry's purity verdict in the
                // first place - a *different* question (does *this* call site
                // definitely reach `sig` itself, not some subclass's own
                // override of it). Only handled for a plain `Object`
                // receiver, matching this whole feature's own disclosed
                // scope (a `Union`/`Intersection` receiver is treated
                // conservatively - not flagged - the same "don't guess"
                // default everywhere else in this feature).
                let Some((recv_class, _)) = recv_ty.as_object() else {
                    return;
                };
                if !crate::registry::is_non_overridable(
                    self.registry,
                    recv_class,
                    &owner,
                    method,
                    sig,
                ) {
                    return;
                }
                (
                    format!("Call to method {owner}::{method}() on a separate line has no effect."),
                    "method.resultUnused",
                )
            }
            ExprKind::StaticCall(target, method, _) => {
                // Real PHPStan's `MethodWithoutImpurePointsCollector`
                // explicitly excludes constructors
                // (`if ($method->isConstructor()) return null;`) -
                // `parent::__construct()`/`self::__construct()` is only
                // ever checked through the `new Foo();`-statement shape
                // (`ConstructorWithoutImpurePointsCollector`), never
                // through this static-call syntax. PHP method names are
                // case-insensitive, hence `eq_ignore_ascii_case` (already
                // the established idiom for constructor-name detection
                // elsewhere, e.g. `checker/phpstan_api.rs`,
                // `checker/duplicates.rs`).
                if syntax::ast::is_constructor_name(method) {
                    return;
                }
                let Some(resolved_class) = self.infer_static_target_class(target, locals) else {
                    return;
                };
                let Some((owner, _sig)) = self
                    .registry
                    .lookup_method_with_owner(&resolved_class, method)
                else {
                    return;
                };
                if !self.registry.method_is_pure(&owner, method) {
                    return;
                }
                (
                    format!("Call to {owner}::{method}() on a separate line has no effect."),
                    "staticMethod.resultUnused",
                )
            }
            ExprKind::New(_, _) => {
                let new_ty = self.infer_expr(e, locals);
                let Some((class, _)) = new_ty.as_object() else {
                    return;
                };
                let Some(_sig) = self.registry.lookup_method(class, "__construct") else {
                    return;
                };
                if !self.registry.method_is_pure(class, "__construct") {
                    return;
                }
                (
                    format!("Call to new {class}() on a separate line has no effect."),
                    "new.resultUnused",
                )
            }
            _ => return,
        };
        self.report(message, e.span, 4, identifier);
    }

    /// `@phpstan-pure`/`@phpstan-impure` tag validation - real PHPStan's
    /// own `FunctionPurityCheck`, scoped to the one direction this
    /// project's own binary purity classification
    /// (the registry's ID-keyed purity verdict - deliberately not the
    /// richer per-point `certain`/`maybe` taxonomy real PHPStan's own
    /// version needs) can support without a real risk of false
    /// positives: a declaration tagged `@phpstan-impure` that this
    /// project's own classifier proves pure
    /// (`impureFunction.pure`/`impureMethod.pure`, matching real
    /// PHPStan's own identical identifier/message pair). The opposite
    /// direction (`@phpstan-pure` tagged on something the classifier
    /// calls impure) is deliberately not built - `is_transitively_pure
    /// == false` conflates "genuinely impure" with "unknown, so
    /// conservatively disqualified" (an unresolvable receiver
    /// unconditionally disqualifies a declaration here, per this
    /// project's own disclosed precision limit versus real PHPStan), so
    /// flagging that direction would misfire on every actually-pure
    /// function this project simply can't prove pure - the same "favor
    /// false negatives" precedent this project holds throughout, applied
    /// to which direction of a two-way check is safe to build at all,
    /// not just how each one individually is worded.
    pub(crate) fn check_declared_purity_tag(
        &mut self,
        f: &FunctionDecl,
        sig: Option<&FunctionReflection>,
        class_name: Option<&str>,
        label: &str,
    ) {
        if sig.is_none() {
            return;
        }
        let is_pure = if let Some(class_name) = class_name {
            self.registry.method_is_pure(class_name, f.name.as_ref())
        } else {
            self.registry.function_is_pure(f.name.as_ref())
        };
        if !is_pure {
            return;
        }
        let Some(doc) = f.doc_comment.as_deref() else {
            return;
        };
        if !doc.contains("@phpstan-impure") && !doc.contains("@impure") {
            return;
        }
        let block = docblock::parse_shared(doc);
        if !block.impure {
            return;
        }
        let (description, identifier) = if class_name.is_some() {
            ("Method", "impureMethod.pure")
        } else {
            ("Function", "impureFunction.pure")
        };
        self.report(
            format!(
                "{description} {label}() is marked as impure but does not have any side effects."
            ),
            f.name.span,
            0,
            identifier,
        );
    }

    /// `Rules/Pure/FunctionPurityCheck.php`'s own
    /// remaining three diagnostics for a declaration real PHPStan
    /// considers pure - `pure{Function,Method}.parameterByRef`,
    /// `pure{Function,Method}.void`, and per-construct `impure.<id>`/
    /// `possiblyImpure.<id>` (`registry::collect_impure_points`'s own
    /// doc comment has the full identifier/certain-vs-maybe mapping).
    ///
    /// Gated on an explicit `@phpstan-pure`/`@pure` tag only, not
    /// the purity verdict alone: real PHPStan's own reflection
    /// layer can infer purity without any tag at all, but doing the same
    /// here would make every one of these four diagnostics fire on any
    /// ordinary, untagged void function/method the classifier happens to
    /// prove pure (an extremely common, unremarkable shape - a short
    /// helper with no observable side effects, tagged or not), for a
    /// real void method nobody ever meant to annotate as
    /// `@phpstan-pure`. Requiring the tag matches this same
    /// file's own `check_declared_purity_tag`'s already-working
    /// precedent (also tag-gated, not classifier-alone) and is the only
    /// way to keep this new family from being a broad, unwanted-by-
    /// default noise source rather than the `@phpstan-pure`-authoring-
    /// aid it's meant to be.
    ///
    /// One real, disclosed consequence of the tag-only gate, worth
    /// stating plainly: `impure.<id>`/`possiblyImpure.<id>` can *only*
    /// ever be non-vacuous for a declaration this project's own
    /// classifier does *not* independently confirm pure (`registry::
    /// purity`'s own fixed point is short-circuiting - the moment it
    /// finds one disqualifying construct the whole declaration is
    /// marked "can never be proven pure" and stops, so `sig.is_
    /// transitively_pure == true` already implies zero disqualifying
    /// constructs exist in the body at all) - i.e. exactly the
    /// `@phpstan-pure`-tagged-but-actually-impure case, the same
    /// scenario `impureFunction.pure`'s own opposite-direction sibling
    /// already covers. `pureFunction.parameterByRef` has the identical
    /// property for a different reason (a by-ref parameter
    /// unconditionally disqualifies `is_transitively_pure` outright,
    /// `purity_gate_passes`) - only `pure{Function,Method}.void` can
    /// fire for a declaration that's *genuinely, correctly* pure (a
    /// real, intended "you tagged this pure and it's void - the tag
    /// adds nothing real PHPStan callers would ever observe" nudge).
    pub(crate) fn check_purity_impure_points(
        &mut self,
        f: &FunctionDecl,
        sig: Option<&FunctionReflection>,
        is_method: bool,
        label: &str,
        implicit_calls: &[syntax::span::Span],
    ) {
        let Some(sig) = sig else {
            return;
        };
        let tagged_pure = f
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("pure") && docblock::parse_shared(doc).pure);
        if !tagged_pure {
            return;
        }
        let (kind_word, identifier_prefix) = if is_method {
            ("Method", "pureMethod")
        } else {
            ("Function", "pureFunction")
        };
        for param in &f.params {
            if param.by_ref {
                self.report(
                    format!(
                        "Parameter ${} of pure {} {label}() is passed by reference.",
                        param.name,
                        kind_word.to_lowercase()
                    ),
                    param.name.span,
                    0,
                    format!("{identifier_prefix}.parameterByRef"),
                );
            }
        }
        if sig.return_type().is_void()
            && !f.is_constructor()
            && sig.throws().is_none()
            && sig.assert().is_empty()
            && sig.assert_if_true().is_empty()
            && sig.assert_if_false().is_empty()
        {
            self.report(
                format!("{kind_word} {label}() is pure but has no return value."),
                f.name.span,
                0,
                format!("{identifier_prefix}.void"),
            );
        }
        let class_name = if is_method {
            label.split("::").next()
        } else {
            None
        };
        for occ in collect_impure_points(f.body_stmts(), class_name, self.registry, implicit_calls)
        {
            let (verb, identifier) = if occ.certain {
                ("Impure", format!("impure.{}", occ.identifier))
            } else {
                (
                    "Possibly impure",
                    format!("possiblyImpure.{}", occ.identifier),
                )
            };
            self.report(
                format!(
                    "{verb} {} in pure {} {label}().",
                    occ.description,
                    kind_word.to_lowercase()
                ),
                occ.span,
                0,
                identifier.clone(),
            );
        }
    }

    /// Declaration-time validation for `@param-immediately-
    /// invoked-callable`/`@param-later-invoked-callable`
    /// (`IncompatibleParamImmediatelyInvokedCallableRule`) and
    /// `@param-closure-this` (`IncompatiblePhpDocTypeCheck`'s own
    /// closure-this half) - does the tag reference a real parameter, and
    /// is that parameter's own *native* type (not the phpdoc-merged one -
    /// a self-contradicting docblock like `@param int $x` +
    /// `@param-immediately-invoked-callable $x` must not silently pass)
    /// structurally compatible with what the tag claims. The runtime
    /// effects these tags separately drive (scope-threading for the
    /// former, `$this`-injection for the latter) are unrelated to this
    /// function - see `registry::function_sig::ParameterReflection::
    /// closure_this_type` for the latter's own runtime half.
    pub(crate) fn check_declared_param_callable_tags(&mut self, f: &FunctionDecl) {
        let Some(doc) = f.doc_comment.as_deref() else {
            return;
        };
        if !doc.contains("param-immediately-invoked-callable")
            && !doc.contains("param-later-invoked-callable")
            && !doc.contains("param-closure-this")
        {
            return;
        }
        let block = docblock::parse_shared(doc);
        for (name, immediately) in &block.param_immediately_invoked_callable {
            let tag = if *immediately { "immediately" } else { "later" };
            let Some(param) = f.params.iter().find(|p| *p.name == **name) else {
                self.report(format!(
                        "PHPDoc tag @param-{tag}-invoked-callable references unknown parameter: ${name}"
                    ),
                    f.name.span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self.resolve_type_hint(param.type_hint.as_ref());
            if !self.param_type_could_satisfy(&native_ty, &Type::callable(None)) {
                let identifier = if *immediately {
                    "paramImmediatelyInvokedCallable.nonCallable"
                } else {
                    "paramLaterInvokedCallable.nonCallable"
                };
                self.report(format!(
                        "PHPDoc tag @param-{tag}-invoked-callable is for parameter ${name} with non-callable type {native_ty}."
                    ),
                    f.name.span,
                    2,
                    identifier,
                );
            }
        }
        for name in block.param_closure_this.keys() {
            let Some(param) = f.params.iter().find(|p| *p.name == **name) else {
                self.report(
                    format!("PHPDoc tag @param-closure-this references unknown parameter: ${name}"),
                    f.name.span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self.resolve_type_hint(param.type_hint.as_ref());
            let closure_ty = Type::object("Closure".into(), Vec::new());
            if !self.param_type_could_satisfy(&native_ty, &closure_ty) {
                self.report(format!(
                        "PHPDoc tag @param-closure-this is for parameter ${name} with non-Closure type {native_ty}."
                    ),
                    f.name.span,
                    2,
                    "paramClosureThis.nonClosure",
                );
            }
        }
    }

    /// Whether `ty` could ever satisfy `target`: `mixed` (an untyped
    /// parameter) could be anything, and a union could when any member
    /// is accepted, not only when all are (a `?callable` parameter).
    pub(crate) fn param_type_could_satisfy(&self, ty: &Type, target: &Type) -> bool {
        ty.is_mixed()
            || ty
                .union_members()
                .iter()
                .any(|m| self.accepts(target, m).yes())
    }
}
