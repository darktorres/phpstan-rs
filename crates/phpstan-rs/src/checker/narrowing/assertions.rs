use crate::checker::Discriminant;
use crate::checker::narrowing::predicates::{narrowing_infer_target, narrowing_key};
use crate::checker::{Checker, Locals};
use syntax::ast::*;
use types::Type;

impl<'a> Checker<'a> {
    /// The type to narrow *from* for a synthetic `narrowing_key` (a
    /// property chain or literal-key array-index chain, never a bare
    /// variable) - `locals.get(key)` alone, defaulting to `mixed`
    /// when absent, is wrong for these: unlike a bare variable (already
    /// tracked in `locals` by ordinary assignment/definition), a
    /// property/index chain's key only ever exists in `locals` as the
    /// result of an *earlier* narrowing - the common case, a chain never
    /// narrowed before, would silently discard its real, already-known
    /// declared/inferred type (from `infer_expr`) in favor of a bogus
    /// `Mixed`, which then gets *narrowed* (a no-op on `Mixed`) and
    /// written back, permanently overriding the correct type for the
    /// rest of the enclosing scope. E.g. `if ($catListRequest->deleteId
    /// !== null) { ... }` narrowing `?int` down to `int` only works if
    /// the "current" type
    /// it narrows from is the real `?int`, not a wrongly-defaulted
    /// `Mixed`. A bare variable already goes through `locals.get(name)`
    /// correctly (it's always present by the time a condition checks
    /// it), so this only matters for non-`Variable` `narrowing_key`
    /// shapes - but computing it uniformly via `expr`/`infer_expr` is
    /// correct for a bare variable too (`infer_expr` on a `Variable`
    /// just reads the same `locals` entry), so every `apply_narrowing`
    /// call site uses this instead of a raw `locals.get(key).unwrap_or
    /// (mixed)`.
    /// `Cow`, not an owned `Type` - every one of this function's ~19 call
    /// sites in `checker/narrowing.rs` only ever reads the result as
    /// `&current` afterward (feeding it into `Relation::narrow`/
    /// `Type::without`/`Relation::narrow_to_instance`/etc, each of which
    /// already takes `&Type` and produces a fresh owned `Type` of their
    /// own), never needing ownership of `current` itself. Per this
    /// function's own doc comment just above, `locals.get(key)` hits on
    /// the overwhelmingly common bare-variable case (already tracked in
    /// `locals` by the time a condition narrows it), so it's the `Some`
    /// branch, not the rarer "first-ever narrowing of a property/index
    /// chain" `infer_expr` fallback, that this avoids cloning for.
    pub(crate) fn current_narrowed_type<'l>(
        &self,
        expr: &Expr,
        key: &str,
        locals: &'l Locals,
    ) -> std::borrow::Cow<'l, Type> {
        match locals.get(key) {
            Some(ty) => std::borrow::Cow::Borrowed(ty),
            None => std::borrow::Cow::Owned(self.infer_expr(narrowing_infer_target(expr), locals)),
        }
    }

    /// The union of `in_array()`'s own haystack literal values, if every
    /// element is a bare, unkeyed literal (`int`/`string`/`bool`/`null`),
    /// or a bare empty-array literal `[]` (just as statically provable a
    /// single exact type, `array{}`, as any scalar
    /// literal - e.g. `! in_array($raw_tags_post, [null, '0', '', []],
    /// true)`'s 4th haystack element being a non-`Literal` `ArrayLiteral`
    /// must not bail this whole function to `None`, silently narrowing
    /// none of the other three, perfectly literal, values either).
    /// `None` if any element is a spread, a
    /// keyed entry, a non-empty array literal, or anything other than one
    /// of those two provable shapes, since this project has no way to
    /// prove any other haystack element's value statically. Reuses
    /// `infer_expr` on each element rather than duplicating the `Literal
    /// -> Type` mapping that already exists, verbatim, in `check_expr`'s
    /// and `infer_expr`'s own `Literal` arms.
    /// A strict `in_array($needle, $haystack, true)`'s narrowing of
    /// `$needle`: the type it has when found, and - only when every
    /// possible haystack value is a single known value (a literal list,
    /// a class constant holding one) - the type it can't have when not
    /// found. A haystack of any other known array narrows the found
    /// branch to its element type.
    pub(crate) fn in_array_narrowing(
        &self,
        haystack: &Expr,
        locals: &Locals,
    ) -> Option<(Type, Option<Type>)> {
        if let ExprKind::ArrayLiteral(items) = &haystack.peeled().kind
            && let Some(target) = self.in_array_target_type(items, locals)
        {
            let excluded = single_values_of(&target);
            return Some((target, excluded));
        }
        let haystack_ty = self.infer_expr(haystack, locals);
        if let [shape] = haystack_ty.constant_arrays()[..]
            && !shape.fields.is_empty()
            && shape.fields.iter().all(|f| f.ty.is_single_value())
        {
            let mut values: Vec<Type> = Vec::new();
            for f in &shape.fields {
                if !values.contains(&f.ty) {
                    values.push(f.ty.clone());
                }
            }
            let target = if values.len() == 1 {
                values.pop().unwrap()
            } else {
                self.relation().union_of(values)
            };
            return Some((target.clone(), Some(target)));
        }
        let element = haystack_ty.array_value_type()?;
        if element.is_mixed() {
            return None;
        }
        Some((element, None))
    }

    pub(crate) fn in_array_target_type(
        &self,
        items: &[ArrayItem],
        locals: &Locals,
    ) -> Option<Type> {
        let mut member_types: Vec<Type> = Vec::new();
        for item in items {
            if item.spread || item.key.is_some() {
                return None;
            }
            let ty = match &item.value.kind {
                ExprKind::Literal(_) => self.infer_expr(&item.value, locals),
                ExprKind::ArrayLiteral(inner) if inner.is_empty() => {
                    Type::array_shape(types::Shape::sealed(Vec::new()))
                }
                _ => return None,
            };
            if !member_types.contains(&ty) {
                member_types.push(ty);
            }
        }
        match member_types.len() {
            0 => None,
            1 => member_types.into_iter().next(),
            _ => Some(self.relation().union_of(member_types)),
        }
    }

    pub(crate) fn apply_equality_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        locals: &mut Locals,
        matches_when_true: bool,
    ) {
        // `narrowing_key` on each side, not a bare `ExprKind::
        // Variable` match - the same array-index gap the `Call` arm above
        // has, for the `$arr['key'] === null` idiom instead of an
        // `is_*()` predicate.
        let pair = match (narrowing_key(l), narrowing_key(r)) {
            (Some(key), _) => Some((key, l, self.infer_expr(r, locals))),
            (None, Some(key)) => Some((key, r, self.infer_expr(l, locals))),
            (None, None) => None,
        };
        // A nullsafe *method call* (`$a?->b()`) has no `narrowing_key`
        // of its own at all (unlike a nullsafe property access, which
        // does) - `pair` above is `None` for it, so the nullsafe-
        // receiver implication below needs its own direct check against
        // `l`/`r`, independent of whether `pair` succeeded, rather than
        // being reached only through `narrowed_expr`.
        if !matches_when_true {
            let nullsafe_side = match (super::nullsafe_receiver(l), super::nullsafe_receiver(r)) {
                (Some(receiver), _) if self.infer_expr(r, locals).is_null().yes() => Some(receiver),
                (_, Some(receiver)) if self.infer_expr(l, locals).is_null().yes() => Some(receiver),
                _ => None,
            };
            if let Some(receiver) = nullsafe_side
                && let Some(receiver_key) = narrowing_key(receiver)
            {
                let current = self.current_narrowed_type(receiver, &receiver_key, locals);
                locals.insert(receiver_key, current.without(&Type::null()));
            }
        }
        let Some((key, narrowed_expr, target_ty)) = pair else {
            return;
        };
        if target_ty.is_mixed() {
            return;
        }
        let current = self.current_narrowed_type(narrowed_expr, &key, locals);
        // `Type::without` is only sound when `excluded`
        // represents one specific, already-proven value (`Null`,
        // `True`/`False`, a literal, an `is_*()` predicate's own
        // provably-exhaustive target) - every one of its other real
        // callers passes exactly that. A general type like plain `Bool`
        // covers multiple distinct values, so `$x !== $y` where `$y`'s
        // only known fact is "some `Bool`" proves nothing about which
        // specific value `$x` isn't - excluding the whole `Bool` type
        // from an already-`Bool`-typed `$x` (e.g. `function
        // test(?bool $a, bool $b) { if ($a !== null && $a !== $b) { ... }
        // }`) would wrongly collapse it to `never`.
        // `Relation::narrow` meets `current` with `target_ty`, sound for
        // a literal and a general type alike (`===` between incompatible
        // types leaves `never`: the branch is dead), so only the
        // exclusion branch needs gating here.
        let narrowed = if matches_when_true {
            self.relation().narrow(&current, &target_ty)
        } else if target_ty.is_single_value() {
            self.exclude(&current, &target_ty)
        } else {
            current.into_owned()
        };
        // A same-branch discriminant-variable correlation
        // (`record_branch_implied_facts`'s own doc comment): this exact
        // discriminant, proven true here, was already seen once to also
        // imply some *other* variable's own non-null type elsewhere in
        // the program - replay those facts too, not just `key`'s own
        // ordinary narrowing above.
        if matches_when_true
            && let Some(implied) = locals
                .branch_implied_facts()
                .get(&Discriminant::new(&key, &target_ty))
                .cloned()
        {
            for (implied_key, implied_ty) in implied.iter() {
                locals.insert(implied_key.clone(), implied_ty.clone());
            }
        }
        // A nullsafe chain assigned to a variable, then tested later
        // (`$x = $a?->b(); if ($x !== null)`) - the direct-testing form
        // (`if ($a?->b() !== null)`, with no intermediate variable) is
        // already handled above, before `pair`'s own early return, since
        // a nullsafe *method* call has no `narrowing_key` of its own to
        // reach `narrowed_expr` through. This replays through `key`'s
        // own `condition_aliases` entry, whose target was stored as the
        // *receiver*'s key already (`condition_alias_targets`'s own
        // nullsafe arm - always exactly one target for this shape), not
        // the chain's.
        if !matches_when_true
            && target_ty.is_null().yes()
            && let Some((alias_targets, alias_expr)) = locals.condition_aliases().get(&key).cloned()
            && let Some(receiver) = super::nullsafe_receiver(&alias_expr)
            && let [alias_target] = alias_targets.as_slice()
        {
            let current = self.current_narrowed_type(receiver, alias_target, locals);
            locals.insert(alias_target.clone(), current.without(&Type::null()));
        }
        locals.insert(key, narrowed);
        // With keys on both sides, the right one narrows against the left.
        if narrowing_key(l).is_some()
            && let Some(r_key) = narrowing_key(r)
        {
            let l_ty = self.infer_expr(l, locals);
            if !l_ty.is_mixed() {
                let current = self.current_narrowed_type(r, &r_key, locals);
                let narrowed = if matches_when_true {
                    self.relation().narrow(&current, &l_ty)
                } else if l_ty.is_single_value() {
                    self.exclude(&current, &l_ty)
                } else {
                    current.into_owned()
                };
                locals.insert(r_key, narrowed);
            }
        }
    }

    /// `l == r` (`equal`) or `l != r` narrows each side with a key
    /// against the other side's type, as PHP's loose comparison allows.
    pub(crate) fn apply_loose_equality_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        locals: &mut Locals,
        equal: bool,
    ) {
        for (side, other) in [(l, r), (r, l)] {
            let Some(key) = narrowing_key(side) else {
                continue;
            };
            let other_ty = self.infer_expr(other, locals);
            if other_ty.is_mixed() {
                continue;
            }
            let current = self.current_narrowed_type(side, &key, locals);
            let narrowed = self.relation().narrow_loose(
                &current,
                &other_ty,
                equal,
                self.comparison_php_version(),
            );
            locals.insert(key, narrowed);
        }
    }

    /// `l op r` (`holds`) narrows each side with a key against the other,
    /// the right side with the operator mirrored, through the comparison
    /// model.
    pub(crate) fn apply_ordering_narrowing(
        &self,
        l: &Expr,
        r: &Expr,
        op: types::OrderingOp,
        holds: bool,
        locals: &mut Locals,
    ) {
        use types::OrderingOp::*;
        let mirrored = match op {
            Lt => Gt,
            LtEq => GtEq,
            Gt => Lt,
            GtEq => LtEq,
        };
        let version = self.comparison_php_version();
        for (side, other, side_op) in [(l, r, op), (r, l, mirrored)] {
            let Some(key) = narrowing_key(side) else {
                continue;
            };
            let other_ty = self.infer_expr(other, locals);
            if other_ty.is_mixed() {
                continue;
            }
            let current = self.current_narrowed_type(side, &key, locals);
            let narrowed = self
                .relation()
                .narrow_ordering(&current, side_op, &other_ty, holds, version);
            locals.insert(key, narrowed);
        }
    }

    /// The PHP version comparisons are judged on: the configured range's,
    /// when it lies on one side of 8.0 (where PHP 7 and 8 compare a number
    /// and a non-numeric string differently), `None` for both.
    pub(crate) fn comparison_php_version(&self) -> Option<types::PhpVersion> {
        let eight = types::PhpVersion::new(8, 0);
        match (
            self.registry.php_version_min(),
            self.registry.php_version_max(),
        ) {
            (Some(min), _) if min >= eight => Some(min),
            (_, Some(max)) if max < eight => Some(max),
            _ => None,
        }
    }

    /// Whether a method/property is confirmed missing from every value
    /// `ty` could hold - `has_member(class)` decides for one concrete
    /// class (already covering whatever fallback, like `__call`/`__get`,
    /// makes a class "have" the member without declaring it directly). A
    /// bare `Object` is the base case; a `Union` value could genuinely be
    /// any member, so missing is confirmed as soon as *any* resolvable
    /// member lacks it; a value satisfying an `Intersection` has every
    /// member's capabilities at once, so missing is only confirmed when
    /// *every* member lacks it. Anything else (a type this checker can't
    /// resolve into class members at all, or a non-`Object` member found
    /// while walking a `Union`/`Intersection`) never confirms "missing" -
    /// this project's consistent preference for fewer false positives
    /// over catching everything.
    pub(crate) fn member_confirmed_missing(
        &self,
        ty: &Type,
        has_member: impl Fn(&str) -> bool + Copy,
    ) -> bool {
        syntax::stack::guard(|| {
            let members = ty.union_members();
            if members.len() > 1 {
                return members
                    .iter()
                    .any(|m| self.member_confirmed_missing(m, has_member));
            }
            let parts = ty.intersection_members();
            if parts.len() > 1 {
                return parts
                    .iter()
                    .all(|m| self.member_confirmed_missing(m, has_member));
            }
            // An enum case is an instance of its enum for member lookup.
            match ty.object_class_names()[..] {
                [class] => self.registry.classes.contains_key(class.as_str()) && !has_member(class),
                _ => false,
            }
        })
    }

    /// Applies one `@phpstan-assert`-family list (`sig.assert_if_true`,
    /// `sig.assert_if_false`, or `sig.assert`) to `locals` - shared by
    /// both the conditional (`apply_narrowing`'s own `MethodCall`/`Call`/
    /// `StaticCall` arms, gated on which branch actually held) and
    /// unconditional (`check_stmt`'s own bare `@phpstan-assert` wiring,
    /// mirroring `assert($cond)`'s identical "narrows for the rest of
    /// scope, no branch/merge needed" shape) call sites. `recv` is the
    /// call's own receiver expression (`Some` for a method call, `None`
    /// for a plain function call) - a `ThisSelf`/`ThisProp` target is
    /// silently skipped when there isn't one, since there's no `$this`
    /// to narrow from a plain function call.
    pub(crate) fn apply_assert_infos(
        &self,
        infos: &[crate::registry::AssertInfo],
        sig: &crate::registry::FunctionReflection,
        recv: Option<&Expr>,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        let resolved = crate::checker::resolve_call_args(args, sig.params());
        let (bindings, _) = self.call_template_bindings(sig, &resolved, locals);
        for info in infos {
            let Some((key, current)) =
                self.assert_target_key_and_current(&info.target, sig, recv, &resolved, locals)
            else {
                continue;
            };
            // The asserted type as this call binds its templates.
            let asserted = self.substitute_call_templates(&info.ty, &bindings);
            let narrowed = match (info.equality, info.negated) {
                (false, false) => self.relation().narrow(&current, &asserted),
                (false, true) => self.exclude(&current, &asserted),
                // `=T`: identical to a value of `T`, as `===` narrows.
                (true, false) => self.relation().narrow(&current, &asserted),
                // `!=T`: not identical to it, which excludes only one value.
                (true, true) if asserted.is_single_value() => self.exclude(&current, &asserted),
                (true, true) => continue,
            };
            locals.insert(key, narrowed);
        }
    }

    /// The `(narrowing key, current type)` pair a `@phpstan-assert`
    /// target resolves to at this call site - `None` when the target
    /// can't be resolved here at all (a `ThisSelf`/`ThisProp` target on a
    /// receiverless call, or a `Param` target this call didn't actually
    /// pass an argument for, e.g. an omitted optional parameter).
    /// `Param`/`ThisSelf` both name a *real* argument/receiver
    /// expression already in hand, so `current_narrowed_type` (which
    /// needs a real `Expr` for its own `infer_expr` fallback) applies
    /// directly; `ThisProp` has no such expression to offer (`$this->
    /// prop` is synthesized from docblock text, never a real parsed
    /// node) - its own "not already narrowed" fallback instead looks the
    /// property's declared type up directly from the receiver's own
    /// resolved class, sidestepping the need to fabricate one.
    fn assert_target_key_and_current<'l>(
        &self,
        target: &crate::registry::AssertTarget,
        sig: &crate::registry::FunctionReflection,
        recv: Option<&Expr>,
        resolved: &[Option<&Expr>],
        locals: &'l Locals,
    ) -> Option<(String, std::borrow::Cow<'l, Type>)> {
        use crate::registry::AssertTarget;
        match target {
            AssertTarget::Param(name) => {
                let idx = sig.params().iter().position(|p| p.name() == name)?;
                let arg = (*resolved.get(idx)?)?;
                let key = narrowing_key(arg)?;
                let current = self.current_narrowed_type(arg, &key, locals);
                Some((key, current))
            }
            AssertTarget::ThisSelf => {
                let recv = recv?;
                let key = narrowing_key(recv)?;
                let current = self.current_narrowed_type(recv, &key, locals);
                Some((key, current))
            }
            // Neither carries a real narrowing target at all - see
            // `AssertTarget`'s own doc comment.
            AssertTarget::NotFound(_) | AssertTarget::Unknown(_) => None,
            AssertTarget::ThisProp(prop) => {
                let recv = recv?;
                let base_key = narrowing_key(recv)?;
                let key = format!("{base_key}->{prop}");
                let current = match locals.get(&key) {
                    Some(t) => std::borrow::Cow::Borrowed(t),
                    // `property_access_type` (`checker/expr.rs`, shared
                    // with `ExprKind::PropertyAccess`) recurses through
                    // `Union`/`Intersection` receivers, so this doesn't
                    // discard the property's real type for a
                    // multi-member receiver.
                    None => std::borrow::Cow::Owned(
                        self.property_access_type(&self.infer_expr(recv, locals), prop),
                    ),
                };
                Some((key, current))
            }
        }
    }
}

/// The members of a haystack's value union that are each one exact value:
/// only those can be ruled out when the needle is not found, since a
/// member like `float` (the type of `0.0`) stands for more values than the
/// one the haystack holds.
// Joins literals and enum cases, which fold without the class hierarchy.
#[allow(clippy::disallowed_methods)]
fn single_values_of(target: &Type) -> Option<Type> {
    let members: Vec<Type> = target
        .union_members()
        .iter()
        .filter(|m| m.is_single_value())
        .cloned()
        .collect();
    match members.len() {
        0 => None,
        1 => members.into_iter().next(),
        _ => Some(Type::union_of(members)),
    }
}
