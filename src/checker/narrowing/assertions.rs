use crate::ast::*;
use crate::checker::narrowing::discriminant_canonical_key;
use crate::checker::narrowing::predicates::{narrowing_infer_target, narrowing_key};
use crate::checker::narrowing::type_ops::{
    is_known_single_value, narrow_type_excluding, narrow_type_to,
};
use crate::checker::{Checker, Locals, unwrap_grouping};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// The type to narrow *from* for a synthetic `narrowing_key` (a
    /// property chain or literal-key array-index chain, never a bare
    /// variable) - `locals.get(key)` alone, defaulting to `Type::Mixed(false)`
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
    /// (Type::Mixed(false))`.
    /// `Cow`, not an owned `Type` - every one of this function's ~19 call
    /// sites in `checker/narrowing.rs` only ever reads the result as
    /// `&current` afterward (feeding it into `narrow_type_to`/
    /// `narrow_type_excluding`/`narrow_instanceof_true`/etc, each of which
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
    /// single exact type, `Type::ArrayShape(vec![])`, as any scalar
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
        if let ExprKind::ArrayLiteral(items) = &unwrap_grouping(haystack).kind
            && let Some(target) = self.in_array_target_type(items, locals)
        {
            return Some((target.clone(), Some(target)));
        }
        let haystack_ty = self.infer_expr(haystack, locals);
        if let Type::ArrayShape(fields) = &haystack_ty
            && !fields.is_empty()
            && fields
                .iter()
                .all(|(k, v, _)| k != "..." && is_known_single_value(v))
        {
            let mut values: Vec<Type> = Vec::new();
            for (_, v, _) in fields {
                if !values.contains(v) {
                    values.push(v.clone());
                }
            }
            let target = if values.len() == 1 {
                values.pop().unwrap()
            } else {
                Type::Union(values)
            };
            return Some((target.clone(), Some(target)));
        }
        let element = Self::array_element_type(&haystack_ty)?;
        if matches!(element, Type::Mixed(_)) {
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
                ExprKind::ArrayLiteral(inner) if inner.is_empty() => Type::ArrayShape(vec![]),
                _ => return None,
            };
            if !member_types.contains(&ty) {
                member_types.push(ty);
            }
        }
        match member_types.len() {
            0 => None,
            1 => member_types.into_iter().next(),
            _ => Some(Type::Union(member_types)),
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
                (Some(receiver), _) if self.infer_expr(r, locals) == Type::Null => Some(receiver),
                (_, Some(receiver)) if self.infer_expr(l, locals) == Type::Null => Some(receiver),
                _ => None,
            };
            if let Some(receiver) = nullsafe_side
                && let Some(receiver_key) = narrowing_key(receiver)
            {
                let current = self.current_narrowed_type(receiver, &receiver_key, locals);
                locals.insert(receiver_key, narrow_type_excluding(&current, &Type::Null));
            }
        }
        let Some((key, narrowed_expr, target_ty)) = pair else {
            return;
        };
        if target_ty == Type::Mixed(false) {
            return;
        }
        let current = self.current_narrowed_type(narrowed_expr, &key, locals);
        // `narrow_type_excluding` is only sound when `excluded`
        // represents one specific, already-proven value (`Null`,
        // `True`/`False`, a literal, an `is_*()` predicate's own
        // provably-exhaustive target) - every one of its other real
        // callers passes exactly that. A general type like plain `Bool`
        // covers multiple distinct values, so `$x !== $y` where `$y`'s
        // only known fact is "some `Bool`" proves nothing about which
        // specific value `$x` isn't - excluding the whole `Bool` type
        // from an already-`Bool`-typed `$x` (e.g. `function
        // test(?bool $a, bool $b) { if ($a !== null && $a !== $b) { ... }
        // }`) would wrongly collapse it to `Type::Never`.
        // `narrow_type_to` doesn't share this bug: for a non-`Union`
        // original it always returns `target` outright (matches iff both
        // sides are already type-compatible, since `===` between
        // incompatible static types is dead code either way) - safe
        // regardless of whether `target_ty` is a literal or a general
        // type, so only the exclusion branch needs gating here.
        let narrowed = if matches_when_true {
            narrow_type_to(self.registry, &current, &target_ty)
        } else if is_known_single_value(&target_ty) {
            narrow_type_excluding(&current, &target_ty)
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
                .branch_implied_facts
                .get(&discriminant_canonical_key(&key, &target_ty))
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
            && target_ty == Type::Null
            && let Some((alias_targets, alias_expr)) = locals.condition_aliases.get(&key).cloned()
            && let Some(receiver) = super::nullsafe_receiver(&alias_expr)
            && let [alias_target] = alias_targets.as_slice()
        {
            let current = self.current_narrowed_type(receiver, alias_target, locals);
            locals.insert(
                alias_target.clone(),
                narrow_type_excluding(&current, &Type::Null),
            );
        }
        locals.insert(key, narrowed);
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
        match ty {
            Type::Object(class, _) => {
                self.registry.classes.contains_key(class.as_str()) && !has_member(class)
            }
            // A specific enum case is still a real instance of
            // its enum for member-lookup purposes - only its own case
            // identity (narrowing, `->name`/`->value`) cares which case,
            // never whether some *other* method/property exists.
            Type::EnumCase(fqn, _) => {
                self.registry.classes.contains_key(fqn.as_str()) && !has_member(fqn)
            }
            Type::Union(members) => members
                .iter()
                .any(|m| self.member_confirmed_missing(m, has_member)),
            Type::Intersection(members) => members
                .iter()
                .all(|m| self.member_confirmed_missing(m, has_member)),
            _ => false,
        }
    }

    /// PHPUnit's own narrowing-relevant assertions (`self::assertIsString
    /// ($output)`, `$this->assertNotNull($x)`, ...) - a real, common
    /// idiom (real PHPStan explicitly supports it via its own
    /// `phpstan/phpstan-phpunit` extension, which this project doesn't
    /// depend on, so the narrowing is replicated directly here) - matches
    /// exactly the same "statement narrows the rest of the scope
    /// unconditionally" shape `assert($cond)` already gets in `check_stmt`'s
    /// `StmtKind::Expression` arm: if execution continues past a failing
    /// assertion, PHPUnit's own `AssertionFailedError` would have been
    /// thrown, so there's only one surviving path forward, no branch/merge
    /// needed. Matched purely on method name, not on confirming the
    /// receiver is really a `PHPUnit\Framework\TestCase` - these names are
    /// specific enough that a false match isn't a realistic concern,
    /// matching this project's existing "trust the name" precedent for
    /// `is_string`/`empty`/`isset` and friends.
    ///
    /// Deliberately excludes `assertIsNumeric` - real PHPStan narrows it to
    /// `int|float|numeric-string`, a type this project has no
    /// representation for; narrowing to a plain `int|float` would be
    /// actively wrong (and a new false negative) for the equally-common
    /// numeric-*string* case.
    pub(crate) fn apply_phpunit_assertion_narrowing(
        &self,
        method: &str,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        let narrow_to = |locals: &mut Locals, idx: usize, target: &Type| {
            if let Some(arg) = args.get(idx)
                && let Some(key) = narrowing_key(&arg.value)
            {
                let current = self.current_narrowed_type(&arg.value, &key, locals);
                locals.insert(key, narrow_type_to(self.registry, &current, target));
            }
        };
        let narrow_excluding = |locals: &mut Locals, idx: usize, excluded: &Type| {
            if let Some(arg) = args.get(idx)
                && let Some(key) = narrowing_key(&arg.value)
            {
                let current = self.current_narrowed_type(&arg.value, &key, locals);
                locals.insert(key, narrow_type_excluding(&current, excluded));
            }
        };
        match method {
            "assertIsString" => narrow_to(locals, 0, &Type::String),
            "assertIsArray" => narrow_to(
                locals,
                0,
                &Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(Type::Mixed(false)),
                ),
            ),
            "assertIsInt" | "assertIsInteger" => narrow_to(locals, 0, &Type::Int),
            "assertIsFloat" => narrow_to(locals, 0, &Type::Float),
            "assertIsBool" | "assertIsBoolean" => narrow_to(locals, 0, &Type::Bool),
            "assertIsResource" => {
                narrow_to(locals, 0, &Type::Object("resource".to_string(), Vec::new()));
            }
            "assertNull" => narrow_to(locals, 0, &Type::Null),
            "assertTrue" => narrow_to(locals, 0, &Type::True),
            "assertFalse" => narrow_to(locals, 0, &Type::False),
            "assertNotNull" => narrow_excluding(locals, 0, &Type::Null),
            "assertNotFalse" => narrow_excluding(locals, 0, &Type::False),
            "assertNotTrue" => narrow_excluding(locals, 0, &Type::True),
            // `assertInstanceOf(string $expected, mixed $actual)` - note
            // the argument order (expected class first, actual value
            // second), the reverse of `instanceof`'s own left-to-right
            // reading - `$expected` is conventionally `Foo::class`, a
            // `StaticAccess(class_name, "class")` expression.
            "assertInstanceOf" => {
                if let Some(class_arg) = args.first()
                    && let ExprKind::StaticAccess(class_name, member) = &class_arg.value.kind
                    && member == "class"
                    && let Some(actual_arg) = args.get(1)
                    && let Some(key) = narrowing_key(&actual_arg.value)
                {
                    locals.insert(key, self.resolve_name_expr(class_name));
                }
            }
            _ => {}
        }
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
        sig: &crate::registry::FunctionSig,
        recv: Option<&Expr>,
        args: &[Arg],
        locals: &mut Locals,
    ) {
        for info in infos {
            let Some((key, current)) =
                self.assert_target_key_and_current(&info.target, sig, recv, args, locals)
            else {
                continue;
            };
            let narrowed = if info.negated {
                narrow_type_excluding(&current, &info.ty)
            } else {
                narrow_type_to(self.registry, &current, &info.ty)
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
        sig: &crate::registry::FunctionSig,
        recv: Option<&Expr>,
        args: &[Arg],
        locals: &'l Locals,
    ) -> Option<(String, std::borrow::Cow<'l, Type>)> {
        use crate::registry::AssertTarget;
        match target {
            AssertTarget::Param(name) => {
                let idx = sig.params.iter().position(|p| &p.name == name)?;
                let arg = args.get(idx)?;
                let key = narrowing_key(&arg.value)?;
                let current = self.current_narrowed_type(&arg.value, &key, locals);
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
