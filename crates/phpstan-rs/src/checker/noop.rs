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
use crate::diagnostic::DiagnosticBuilder;
use crate::registry::{FunctionSig, collect_impure_points};
use phpdoc::docblock;
use syntax::ast::TypeHint;
use syntax::ast::{BinOp, CastKind, Expr, ExprKind, FunctionDecl, UnaryOp};
use syntax::visit::child_exprs;
use types::Type;

const PRECEDENCE_TIP: &str =
    "This operator has unexpected precedence, try disambiguating the logic with parentheses ().";

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Effects {
    None,
    LocalAssignOnly,
    Some,
}

fn is_pure_language_construct(callee: &Expr) -> bool {
    matches!(&callee.kind, ExprKind::Name(n) if n.eq_ignore_ascii_case("isset") || n.eq_ignore_ascii_case("empty"))
}

fn is_local_assign_target(target: &Expr) -> bool {
    match &target.kind {
        ExprKind::Variable(name) => name.as_ref() != "this",
        ExprKind::ArrayLiteral(items) => items.iter().all(|item| {
            item.key
                .as_ref()
                .is_none_or(|k| expr_side_effects(k) == Effects::None)
                && is_local_assign_target(&item.value)
        }),
        _ => false,
    }
}

/// The strongest effect anywhere in `e`'s subtree. A nested closure or
/// anonymous class's body doesn't run by being constructed, so it
/// doesn't count.
fn expr_side_effects(e: &Expr) -> Effects {
    let own = match &e.kind {
        ExprKind::Call(callee, _) if is_pure_language_construct(callee) => Effects::None,
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
        | ExprKind::DynamicVariable(..)
        | ExprKind::Unary(
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
            _,
        ) => Effects::Some,
        _ => Effects::None,
    };
    if own == Effects::Some {
        return own;
    }
    child_exprs(e)
        .into_iter()
        .map(expr_side_effects)
        .fold(own, std::cmp::max)
}

impl<'a> Checker<'a> {
    /// Effects that depend on the operands' types: `__get`/`get` hooks on
    /// a property read, `offsetGet` on an object offset read, and
    /// `__toString` on a stringified object.
    fn expr_has_hidden_effects(&self, e: &Expr, locals: &Locals) -> bool {
        let own = match &e.kind {
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
        };
        own || child_exprs(e)
            .into_iter()
            .any(|child| self.expr_has_hidden_effects(child, locals))
    }

    fn property_read_is_plain(&self, receiver: &Type, name: &str) -> bool {
        match receiver {
            Type::Object(class, _) => {
                if self.registry.lookup_method(class, "__get").is_some() {
                    return false;
                }
                match self.registry.lookup_property(class, name) {
                    Some(prop) => !prop.has_get_hook,
                    None => self.registry.lookup_class(class).is_some(),
                }
            }
            Type::EnumCase(..) => true,
            Type::Union(members) => members
                .iter()
                .all(|m| matches!(m, Type::Null) || self.property_read_is_plain(m, name)),
            _ => false,
        }
    }

    fn offset_read_is_plain(&self, base: &Type) -> bool {
        match base {
            Type::Union(members) => members.iter().all(|m| self.offset_read_is_plain(m)),
            Type::Array(..)
            | Type::ArrayShape(..)
            | Type::String
            | Type::StringLiteral(_)
            | Type::ClassString(_)
            | Type::ClassKindString(_)
            | Type::Accessory(_)
            | Type::Null
            | Type::Bool
            | Type::True
            | Type::False
            | Type::Int
            | Type::IntLiteral(_)
            | Type::IntRange(..)
            | Type::Float
            | Type::FloatLiteral(_) => true,
            _ => false,
        }
    }

    fn may_be_stringable_object(&self, ty: &Type) -> bool {
        match ty {
            Type::Union(members) => members.iter().any(|m| self.may_be_stringable_object(m)),
            Type::Object(..) | Type::ObjectWithoutClass | Type::Intersection(..) => true,
            _ => false,
        }
    }

    /// Called from `StmtKind::Expression` after `e` itself was checked.
    pub(crate) fn check_noop_statement(&mut self, e: &Expr, locals: &Locals) {
        let e = match &e.kind {
            ExprKind::Grouping(inner) => inner.as_ref(),
            _ => e,
        };
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
    /// this reads `is_transitively_pure` straight off whatever
    /// `FunctionSig` the call actually, concretely resolves to, with no
    /// need to reconstruct a `PurityDependency` key at all.
    pub(crate) fn check_pure_call_without_effect(&mut self, e: &Expr, locals: &Locals) {
        let (message, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                let Some(sig) = self.registry.lookup_function(name) else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                (
                    format!("Call to function {name}() on a separate line has no effect."),
                    "function.resultUnused",
                )
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv_ty = self.infer_expr(recv, locals);
                let Some((owner, sig)) = self.find_method_owner(&recv_ty, method) else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                // Real PHPStan's own `PossiblyPureMethodCallCollector`
                // re-applies the identical overridability gate
                // `registry::purity`'s own classification pass already
                // used to decide `sig.is_transitively_pure` in the first
                // place - a *different* question (does *this* call site
                // definitely reach `sig` itself, not some subclass's own
                // override of it). Only handled for a plain `Object`
                // receiver, matching this whole feature's own disclosed
                // scope (a `Union`/`Intersection` receiver is treated
                // conservatively - not flagged - the same "don't guess"
                // default everywhere else in this feature).
                let Type::Object(recv_class, _) = &recv_ty else {
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
                if method.eq_ignore_ascii_case("__construct") {
                    return;
                }
                let Some(resolved_class) = self.infer_static_target_class(target, locals) else {
                    return;
                };
                let Some((owner, sig)) = self
                    .registry
                    .lookup_method_with_owner(&resolved_class, method)
                else {
                    return;
                };
                if !sig.is_transitively_pure {
                    return;
                }
                (
                    format!("Call to {owner}::{method}() on a separate line has no effect."),
                    "staticMethod.resultUnused",
                )
            }
            ExprKind::New(_, _) => {
                let new_ty = self.infer_expr(e, locals);
                let Type::Object(class, _) = &new_ty else {
                    return;
                };
                let Some(sig) = self.registry.lookup_method(class, "__construct") else {
                    return;
                };
                if !sig.is_transitively_pure {
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
    /// (`FunctionSig::is_transitively_pure` - deliberately not the
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
        sig: Option<&FunctionSig>,
        is_method: bool,
        label: &str,
    ) {
        let Some(sig) = sig else {
            return;
        };
        if !sig.is_transitively_pure {
            return;
        }
        let Some(doc) = f.doc_comment.as_deref() else {
            return;
        };
        if !doc.contains("@phpstan-impure") && !doc.contains("@impure") {
            return;
        }
        let block = docblock::parse(doc);
        if !block.impure {
            return;
        }
        let (description, identifier) = if is_method {
            ("Method", "impureMethod.pure")
        } else {
            ("Function", "impureFunction.pure")
        };
        self.report(
            format!(
                "{description} {label}() is marked as impure but does not have any side effects."
            ),
            f.name_span,
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
    /// `sig.is_transitively_pure` alone: real PHPStan's own reflection
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
        sig: Option<&FunctionSig>,
        is_method: bool,
        label: &str,
    ) {
        let Some(sig) = sig else {
            return;
        };
        let tagged_pure = f
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("pure") && docblock::parse(doc).pure);
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
                    param.name_span,
                    0,
                    format!("{identifier_prefix}.parameterByRef"),
                );
            }
        }
        if sig.return_type == Type::Void
            && f.name != "__construct".into()
            && sig.throws.is_none()
            && sig.assert.is_empty()
            && sig.assert_if_true.is_empty()
            && sig.assert_if_false.is_empty()
        {
            self.report(
                format!("{kind_word} {label}() is pure but has no return value."),
                f.name_span,
                0,
                format!("{identifier_prefix}.void"),
            );
        }
        let class_name = if is_method {
            label.split("::").next()
        } else {
            None
        };
        for occ in collect_impure_points(&f.body, class_name, self.registry) {
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
    /// function - see `registry::function_sig::ParamSig::
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
        let block = docblock::parse(doc);
        for (name, immediately) in &block.param_immediately_invoked_callable {
            let tag = if *immediately { "immediately" } else { "later" };
            let Some(param) = f.params.iter().find(|p| *p.name == **name) else {
                self.report(format!(
                        "PHPDoc tag @param-{tag}-invoked-callable references unknown parameter: ${name}"
                    ),
                    f.name_span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self
                .resolve_type_hint(param.type_hint.as_ref().map(TypeHint::to_string).as_deref());
            if !self.param_type_could_satisfy(&native_ty, &Type::Callable(None)) {
                let identifier = if *immediately {
                    "paramImmediatelyInvokedCallable.nonCallable"
                } else {
                    "paramLaterInvokedCallable.nonCallable"
                };
                self.report(format!(
                        "PHPDoc tag @param-{tag}-invoked-callable is for parameter ${name} with non-callable type {native_ty}."
                    ),
                    f.name_span,
                    2,
                    identifier,
                );
            }
        }
        for name in block.param_closure_this.keys() {
            let Some(param) = f.params.iter().find(|p| *p.name == **name) else {
                self.report(
                    format!("PHPDoc tag @param-closure-this references unknown parameter: ${name}"),
                    f.name_span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self
                .resolve_type_hint(param.type_hint.as_ref().map(TypeHint::to_string).as_deref());
            let closure_ty = Type::Object("Closure".into(), Vec::new());
            if !self.param_type_could_satisfy(&native_ty, &closure_ty) {
                self.report(format!(
                        "PHPDoc tag @param-closure-this is for parameter ${name} with non-Closure type {native_ty}."
                    ),
                    f.name_span,
                    2,
                    "paramClosureThis.nonClosure",
                );
            }
        }
    }

    /// Whether `ty` could ever satisfy `target` - deliberately not a
    /// direct `self.registry.accepts(target, ty).yes()` call. `Type::Mixed(false)`
    /// (an untyped parameter) must satisfy anything (real PHPStan's own
    /// `mixed.isCallable()`/`isSuperTypeOf()` is always `maybe`, never
    /// `no`) - `subtype(Mixed, _)` has no such arm and would wrongly
    /// flag every untyped parameter. A `Union` (e.g. `callable|null`) must
    /// satisfy when *any* member does, not when *all* do -
    /// `subtype(Union, _)` requires all members, the right rule for
    /// soundly narrowing a type but the wrong one for this "could this
    /// ever be `target`" query; without this, a real, legal `?callable`
    /// parameter would be wrongly flagged since `Null` alone isn't a
    /// callable/Closure. Both guards protect against real false-positive
    /// classes, not hypothetical ones.
    pub(crate) fn param_type_could_satisfy(&self, ty: &Type, target: &Type) -> bool {
        match ty {
            Type::Mixed(_) => true,
            Type::Union(members) => members
                .iter()
                .any(|m| self.param_type_could_satisfy(m, target)),
            _ => self.registry.accepts(target, ty).yes(),
        }
    }
}
