//! Real PHPStan's `Rules\DeadCode\NoopRule`, narrowed
//! to the slice this project's own AST can support without ambiguity.
//!
//! **The gate that matters most**: `NoopRule` doesn't itself decide
//! whether a bare-statement expression "does nothing" - it only runs at
//! all once `Analyser\StmtHandler\ExpressionHandler` has already
//! established the whole expression has zero impure points and zero
//! explicit throw points. Without an equivalent gate here, this check
//! would misfire on `$cond || $this->die('message');`'s own well-known
//! "guard or die" idiom, whose top-level shape (a bare `||` statement)
//! is otherwise indistinguishable from a genuine no-op.
//! `expr_is_side_effect_free` below is a syntactic approximation of that
//! same "zero impure points" precondition - no attempt at real purity
//! inference (`@phpstan-pure` tags, transitive call-graph purity), just
//! a conservative "does this subtree contain anything that could
//! possibly do something" scan. Conservative in the safe direction
//! throughout: anything this can't prove side-effect-free is treated as
//! side-effecting, never the other way around.
//!
//! Real PHPStan's version additionally splits `and`/`or`
//! (`Node\Expr\BinaryOp\LogicalAnd`/`LogicalOr`) from `&&`/`||`
//! (`BooleanAnd`/`BooleanOr`) into two different identifiers with two
//! different *additional* gates on top of the shared purity one above:
//! `logicalAnd.resultUnused`/`logicalOr.resultUnused` fire regardless of
//! whether the statement also contains an assignment (the whole point,
//! since `$x = a() and b();` parses as `($x = a()) and b();` and
//! silently discards `b()`'s own result - though note `a()`/`b()` being
//! real calls means this exact example still wouldn't reach `NoopRule`
//! at all, per the purity gate above; the distinction only matters for a
//! side-effect-free assignment target, e.g. `$x = true and someCheck();`
//! where `someCheck()` genuinely has no side effects), while
//! `booleanAnd.resultUnused`/`booleanOr.resultUnused` require the
//! *additional* absence of any assignment anywhere in the statement.
//! This project's own parser deliberately erases the `and`/`or`-vs-
//! `&&`/`||` distinction at parse time (see `BinOp::LogicalXor`'s own
//! doc comment in `ast.rs`: "only their *precedence* differs, which is
//! purely a parse-time concern already resolved by the time this tree
//! exists"), so that additional split isn't recoverable here - every
//! `And`/`Or` case below is treated as the `&&`/`||`-shaped one
//! (requiring no assignment anywhere, which `expr_is_side_effect_free`
//! already guarantees, since an assignment is itself a side effect) -
//! a strict, never-wrong subset of the real rule's own combined scope.
//!
//! `xor` (`BinOp::LogicalXor`) has no competing spelling - PHP's `xor`
//! keyword is the only way to write it - so it needs no such
//! disambiguation, just the shared purity gate.
//!
//! Still open: the four `CallToFunctionStatementWithoutSideEffectsRule`/
//! `*MethodStatement*`/`*StaticMethodStatement*`/`*ConstructorStatement*`
//! rules (a call/`new` whose own signature is known to have no side
//! effects - needs real purity data this project doesn't track yet),
//! and the generic "any other side-effect-free bare expression"
//! `expr.resultUnused` fallback (needs a source-expression printer this
//! project doesn't have, to name the useless expression in the
//! message).

use crate::ast::{BinOp, Expr, ExprKind, FunctionDecl, UnaryOp};
use crate::ast_walk::expr_children;
use crate::checker::{Checker, Locals};
use crate::docblock;
use crate::registry::{FunctionSig, collect_impure_points};
use crate::types::Type;

/// Whether `e`'s own subtree provably has no observable side effect at
/// all - see the module doc comment for why this exists and what it
/// approximates. `expr_children`'s own "stops at a nested `Closure`/
/// `ArrowFn`/`NewAnonymousClass` boundary" semantics apply here too
/// (matching `expr_contains_yield`'s identical, already-established
/// precedent): merely *constructing* a closure/anonymous-class value
/// has no side effect of its own - only calling it would, which is a
/// separate expression this same recursion would independently flag if
/// it appeared directly in `e`'s own subtree.
fn expr_is_side_effect_free(e: &Expr) -> bool {
    let self_free = !matches!(
        e.kind,
        ExprKind::Call(..)
            | ExprKind::MethodCall(..)
            | ExprKind::StaticCall(..)
            | ExprKind::DynamicMethodCall(..)
            | ExprKind::New(..)
            | ExprKind::NewAnonymousClass(..)
            | ExprKind::Assign(..)
            | ExprKind::AssignRef(..)
            | ExprKind::CompoundAssign(..)
            | ExprKind::Throw(..)
            | ExprKind::Clone(..)
            | ExprKind::Yield(..)
            | ExprKind::YieldFrom(..)
            | ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                _
            )
    );
    self_free && expr_children(e).into_iter().all(expr_is_side_effect_free)
}

impl<'a> Checker<'a> {
    /// Called from `StmtKind::Expression`'s own handling - `e` is the
    /// bare statement's own top-level expression, already fully checked
    /// by the time this runs (so this only ever *adds* a diagnostic, it
    /// never needs to walk into `e` itself for anything but the
    /// `expr_is_side_effect_free` scan above).
    pub(crate) fn check_noop_statement(&mut self, e: &Expr, label: &str) {
        let (message, identifier) = match &e.kind {
            ExprKind::Binary(BinOp::LogicalXor, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"xor\" operator.".to_string(),
                "logicalXor.resultUnused",
            ),
            ExprKind::Binary(BinOp::And, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"&&\" operator.".to_string(),
                "booleanAnd.resultUnused",
            ),
            ExprKind::Binary(BinOp::Or, ..) if expr_is_side_effect_free(e) => (
                "Unused result of \"||\" operator.".to_string(),
                "booleanOr.resultUnused",
            ),
            ExprKind::Ternary(..) if expr_is_side_effect_free(e) => (
                "Unused result of ternary operator.".to_string(),
                "ternary.resultUnused",
            ),
            _ => return,
        };
        self.report(label.to_string(), message, e.span, 4, identifier);
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
    pub(crate) fn check_pure_call_without_effect(
        &mut self,
        e: &Expr,
        locals: &Locals,
        label: &str,
    ) {
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
        self.report(label.to_string(), message, e.span, 4, identifier);
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
            label.to_string(),
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
        let tagged_pure = f.doc_comment.as_deref().is_some_and(|doc| {
            (doc.contains("@phpstan-pure") || doc.contains("@pure")) && docblock::parse(doc).pure
        });
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
                    label.to_string(),
                    format!(
                        "Parameter ${} of pure {} {label}() is passed by reference.",
                        param.name,
                        kind_word.to_lowercase()
                    ),
                    param.name_span,
                    0,
                    &format!("{identifier_prefix}.parameterByRef"),
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
                label.to_string(),
                format!("{kind_word} {label}() is pure but has no return value."),
                f.name_span,
                0,
                &format!("{identifier_prefix}.void"),
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
                label.to_string(),
                format!(
                    "{verb} {} in pure {} {label}().",
                    occ.description,
                    kind_word.to_lowercase()
                ),
                occ.span,
                0,
                &identifier,
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
    pub(crate) fn check_declared_param_callable_tags(&mut self, f: &FunctionDecl, label: &str) {
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
                self.report(
                    label.to_string(),
                    format!(
                        "PHPDoc tag @param-{tag}-invoked-callable references unknown parameter: ${name}"
                    ),
                    f.name_span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self.resolve_type_hint(param.type_hint.as_deref());
            if !self.param_type_could_satisfy(&native_ty, &Type::Callable(None)) {
                let identifier = if *immediately {
                    "paramImmediatelyInvokedCallable.nonCallable"
                } else {
                    "paramLaterInvokedCallable.nonCallable"
                };
                self.report(
                    label.to_string(),
                    format!(
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
                    label.to_string(),
                    format!("PHPDoc tag @param-closure-this references unknown parameter: ${name}"),
                    f.name_span,
                    2,
                    "parameter.notFound",
                );
                continue;
            };
            let native_ty = self.resolve_type_hint(param.type_hint.as_deref());
            let closure_ty = Type::Object("Closure".to_string(), Vec::new());
            if !self.param_type_could_satisfy(&native_ty, &closure_ty) {
                self.report(
                    label.to_string(),
                    format!(
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
    /// direct `self.registry.is_assignable(ty, target)` call. `Type::Mixed(false)`
    /// (an untyped parameter) must satisfy anything (real PHPStan's own
    /// `mixed.isCallable()`/`isSuperTypeOf()` is always `maybe`, never
    /// `no`) - `is_subtype(Mixed, _)` has no such arm and would wrongly
    /// flag every untyped parameter. A `Union` (e.g. `callable|null`) must
    /// satisfy when *any* member does, not when *all* do -
    /// `is_subtype(Union, _)` requires all members, the right rule for
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
            _ => self.registry.is_assignable(ty, target),
        }
    }
}
