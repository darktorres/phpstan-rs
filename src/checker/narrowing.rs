use crate::ast::*;
use crate::checker::{
    Checker, Locals, literal_string_key, literal_string_value, preg_match_matches_shape,
    union_types,
};
use crate::regex_groups;
use crate::types::{Type, is_subtype};

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
        // Phase 12 Finding E: `preg_match(...)` (bare, or compared
        // against a literal 0/1) doesn't otherwise match any existing
        // narrowing shape below, so this doesn't shadow anything -
        // verified directly, not assumed: the `Binary(op, l, r)` arm
        // further down *does* also run afterward for a comparison form,
        // dispatching to `apply_equality_narrowing`, but that function's
        // own `narrowing_key(l)`/`narrowing_key(r)` calls have no arm for
        // `ExprKind::Call` at all, so it's a genuine no-op for this
        // shape, not a silent double-narrowing.
        if let Some((pattern_arg, matches_arg, true_means_matched)) = preg_match_condition(cond)
            && let ExprKind::Variable(matches_name) = &matches_arg.kind
            && let Some(pattern_str) = literal_string_value(pattern_arg)
            && let Some(groups) = regex_groups::parse_capture_groups(pattern_str)
        {
            let narrowed = if branch_true == true_means_matched {
                preg_match_matches_shape(&groups, &Type::String, false)
            } else {
                Type::ArrayShape(vec![])
            };
            purge_stale_narrowing_for(locals, matches_name);
            locals.insert(matches_name.clone(), narrowed);
        }
        match &cond.kind {
            // `&&` narrows the "both true" branch by composing each side
            // in order (the right operand's own narrowing can rely on the
            // left's, same as check_expr's short-circuit-aware handling)
            // - the false branch would need "at least one is false, but
            // not sure which", which isn't a narrowing this can express,
            // so it's left alone, same conservative choice `||` mirrors
            // for its own unrepresentable branch below.
            ExprKind::Binary(BinOp::And, l, r) if branch_true => {
                self.apply_narrowing(l, locals, true);
                self.apply_narrowing(r, locals, true);
            }
            ExprKind::Binary(BinOp::Or, l, r) if !branch_true => {
                self.apply_narrowing(l, locals, false);
                self.apply_narrowing(r, locals, false);
            }
            // Phase 34 Finding B: "at least one is true, but not sure
            // which" isn't representable *in general* - but when every
            // operand is a direct predicate-table check on the identical
            // expression, it narrows precisely to the union of what each
            // operand would give alone (`is_int($x) || is_string($x)`
            // being true means `$x` is exactly `int|string`, not "give
            // up"). `or_chain_narrowing_target` is the one doing the
            // actual restriction-checking; this arm only acts when it
            // succeeds.
            ExprKind::Binary(BinOp::Or, ..) if branch_true => {
                if let Some((key, target_ty, repr_expr)) = or_chain_narrowing_target(cond) {
                    let current = self.current_narrowed_type(repr_expr, &key, locals);
                    locals.insert(key, narrow_type_to(&current, &target_ty));
                }
            }
            // Deliberately `===`/`!==` only, not `==`/`!=` - a loose
            // comparison matches PHP's own type-juggling table (`0 ==
            // false`, `0 == null`, `"0" == 0`, ...), a much wider set of
            // values than the literal compared-to one, so narrowing a
            // variable down to exactly that value on a loose match would
            // discard every other value `==` could also have matched.
            ExprKind::Binary(op, l, r) => {
                let is_eq = matches!(op, BinOp::Identical);
                let is_neq = matches!(op, BinOp::NotIdentical);
                if is_eq || is_neq {
                    let matches_when_true = if is_eq { branch_true } else { !branch_true };
                    self.apply_equality_narrowing(l, r, locals, matches_when_true);
                }
            }
            ExprKind::Unary(UnaryOp::Not, inner) => {
                self.apply_narrowing(inner, locals, !branch_true);
            }
            ExprKind::Grouping(inner) => self.apply_narrowing(inner, locals, branch_true),
            // Phase 26: `(bool) $x`'s own truthiness is exactly `$x`'s -
            // PHP's cast doesn't change which values are truthy/falsy, it
            // just makes the check explicit (a common, idiomatic style:
            // `if (! (bool) $fp) { throw ...; }` after `fopen()`) - so
            // this narrows precisely as if the cast weren't there at all.
            // Any other cast (`(int)`, `(string)`, ...) genuinely changes
            // the value and isn't narrowable this way, so it's left alone.
            ExprKind::Cast(kind, inner) if kind == "bool" => {
                self.apply_narrowing(inner, locals, branch_true);
            }
            ExprKind::Call(callee, args) => {
                // Phase 22: `narrowing_key`, not a bare `ExprKind::
                // Variable` match - without this, `is_string($arr['key'])`
                // (or `empty($arr['key'])`) never narrowed `$arr['key']`
                // at all, the array-index counterpart of the exact gap
                // the `instanceof` arm below already covers for property
                // chains (`$arg->value instanceof Variable`).
                if let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice())
                    && let Some(key) = narrowing_key(&single_arg.value)
                {
                    if name == "empty" {
                        // `empty($x)` is PHP's own falsy check - only the
                        // branch where it evaluated `false` (`$x` proven
                        // truthy) is safely narrowable, the same asymmetry
                        // as a bare truthiness check.
                        if !branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding_falsy(&current));
                        }
                    } else if name == "isset" {
                        // Phase 23: `isset($x)`'s truthy branch means
                        // specifically "not null" - it's true for `0`,
                        // `''`, `false`, an empty array, every falsy-but-
                        // non-null value - so this narrows only `Null`
                        // away, unlike `empty()`'s full falsy exclusion
                        // above. The false branch isn't safely narrowable
                        // to anything more specific, the same asymmetry
                        // `empty`/the truthy fallback below already accept.
                        if branch_true {
                            let current =
                                self.current_narrowed_type(&single_arg.value, &key, locals);
                            locals.insert(key, narrow_type_excluding(&current, &Type::Null));
                        }
                    } else if name == "is_numeric" {
                        // Phase 32 Finding G: deliberately its own arm,
                        // not a `narrowing_type_for_predicate` table entry
                        // - that table's single target type feeds *both*
                        // directions through the same dispatch below,
                        // which is sound for `is_scalar`/`is_string`/etc.
                        // (a string is unconditionally scalar/a-string
                        // either way) but not for `is_numeric`, where a
                        // string only *sometimes* qualifies: excluding a
                        // shared "int|float|string" target on the false
                        // branch would wrongly drop `String` entirely,
                        // even though a genuinely non-numeric string
                        // (`"hello"`) is exactly the ordinary case that
                        // makes `!is_numeric` true. The two directions
                        // need different exclusion sets instead.
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            // Bool/True/False/Null can never satisfy
                            // is_numeric - unconditionally safe to drop.
                            // String and any numeric type are left
                            // untouched (a string might still be
                            // numeric-looking; this project has no
                            // numeric-string type to narrow down to).
                            [Type::Bool, Type::True, Type::False, Type::Null]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                })
                        } else {
                            // Int/Float are unconditionally numeric, so
                            // failing is_numeric proves the value wasn't
                            // one - nothing else can be dropped this way.
                            [Type::Int, Type::Float]
                                .into_iter()
                                .fold(current, |acc, excluded| {
                                    narrow_type_excluding(&acc, &excluded)
                                })
                        };
                        locals.insert(key, narrowed);
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            narrow_type_to(&current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    }
                } else if let ExprKind::Name(name) = &callee.kind
                    && name == "in_array"
                    && let [needle, haystack, strict] = args.as_slice()
                    // Phase 33 Finding 2: only the strict, literal-array
                    // form real PHPStan's own `InArrayFunctionTypeSpecifyingExtension`
                    // narrows - a loose comparison's type-juggling would
                    // make the real safe target wider than the literal
                    // values listed here.
                    && matches!(&strict.value.kind, ExprKind::Literal(Literal::Bool(true)))
                    && let ExprKind::ArrayLiteral(items) = &haystack.value.kind
                    && let Some(target_ty) = self.in_array_target_type(items, locals)
                    && let Some(key) = narrowing_key(&needle.value)
                {
                    let current = self.current_narrowed_type(&needle.value, &key, locals);
                    let narrowed = if branch_true {
                        narrow_type_to(&current, &target_ty)
                    } else {
                        narrow_type_excluding(&current, &target_ty)
                    };
                    locals.insert(key, narrowed);
                } else if let ExprKind::Name(name) = &callee.kind
                    && name == "array_key_exists"
                    && let [key_arg, arr_arg] = args.as_slice()
                    && let ExprKind::Literal(Literal::String(lit_key)) = &key_arg.value.kind
                    && let Some(arr_key) = narrowing_key(&arr_arg.value)
                {
                    // Phase 33 Finding 4: the checked expression here is
                    // the array argument, not the key - proving/disproving
                    // whether a specific offset is present lets a `Union`
                    // member that could never make the call come out this
                    // way be dropped, the same "keep/drop a Union member
                    // based on a provable fact" shape `instanceof`'s own
                    // member resolution already uses.
                    let current = self.current_narrowed_type(&arr_arg.value, &arr_key, locals);
                    let narrowed =
                        narrow_array_shape_by_key_presence(&current, lit_key, branch_true);
                    locals.insert(arr_key, narrowed);
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // Phase 33 Finding 1: the false branch doesn't need a
                // "not this class" type to be representable - it only
                // needs to *drop* the one member just proven impossible
                // from an already-enumerated `Union`, exactly what
                // `narrow_type_excluding` already does soundly elsewhere.
                // A dynamic target (`$x instanceof $className`) still
                // doesn't name a concrete class to narrow to either way.
                if let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name) = target
                {
                    let resolved = self.resolve_name_expr(class_name);
                    if branch_true {
                        locals.insert(key, resolved);
                    } else {
                        let current = self.current_narrowed_type(inner, &key, locals);
                        locals.insert(key, narrow_type_excluding(&current, &resolved));
                    }
                }
            }
            // A bare truthiness check (`if ($x)` / `if (!$x)`, the latter
            // reaching here via the `Not` arm's recursion above) - only
            // the truthy branch is narrowable, by excluding the falsy
            // members this project can represent (`Null`/`False`); the
            // falsy branch isn't safely narrowable to anything more
            // specific than the current type.
            _ if branch_true && narrowing_key(cond).is_some() => {
                let key = narrowing_key(cond).expect("checked by guard above");
                let current = self.current_narrowed_type(cond, &key, locals);
                locals.insert(key, narrow_type_excluding_falsy(&current));
            }
            _ => {}
        }
    }

    /// The type to narrow *from* for a synthetic `narrowing_key` (a
    /// property chain or literal-key array-index chain, never a bare
    /// variable) - `locals.get(key)` alone, defaulting to `Type::Mixed`
    /// when absent, is wrong for these: unlike a bare variable (already
    /// tracked in `locals` by ordinary assignment/definition), a
    /// property/index chain's key only ever exists in `locals` as the
    /// result of an *earlier* narrowing - the common case, a chain never
    /// narrowed before, would silently discard its real, already-known
    /// declared/inferred type (from `infer_expr`) in favor of a bogus
    /// `Mixed`, which then gets *narrowed* (a no-op on `Mixed`) and
    /// written back, permanently overriding the correct type for the
    /// rest of the enclosing scope. A real corpus example this fix
    /// exists for: `if ($catListRequest->deleteId !== null) { ... }`
    /// narrowing `?int` down to `int` only works if the "current" type
    /// it narrows from is the real `?int`, not a wrongly-defaulted
    /// `Mixed`. A bare variable already goes through `locals.get(name)`
    /// correctly (it's always present by the time a condition checks
    /// it), so this only matters for non-`Variable` `narrowing_key`
    /// shapes - but computing it uniformly via `expr`/`infer_expr` is
    /// correct for a bare variable too (`infer_expr` on a `Variable`
    /// just reads the same `locals` entry), so every `apply_narrowing`
    /// call site uses this instead of a raw `locals.get(key).unwrap_or
    /// (Type::Mixed)`.
    fn current_narrowed_type(&self, expr: &Expr, key: &str, locals: &Locals) -> Type {
        locals
            .get(key)
            .cloned()
            .unwrap_or_else(|| self.infer_expr(narrowing_infer_target(expr), locals))
    }

    /// The union of `in_array()`'s own haystack literal values, if every
    /// element is a bare, unkeyed literal (`int`/`string`/`bool`/`null`) -
    /// `None` if any element is a spread, a keyed entry, or anything other
    /// than a syntactic literal, since this project has no way to prove a
    /// non-literal haystack element's value statically. Reuses `infer_expr`
    /// on each element rather than duplicating the `Literal -> Type`
    /// mapping that already exists, verbatim, in `check_expr`'s and
    /// `infer_expr`'s own `Literal` arms.
    fn in_array_target_type(&self, items: &[ArrayItem], locals: &Locals) -> Option<Type> {
        let mut member_types: Vec<Type> = Vec::new();
        for item in items {
            if item.spread || item.key.is_some() {
                return None;
            }
            if !matches!(item.value.kind, ExprKind::Literal(_)) {
                return None;
            }
            let ty = self.infer_expr(&item.value, locals);
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
        // Phase 22: `narrowing_key` on each side, not a bare `ExprKind::
        // Variable` match - the same array-index gap the `Call` arm above
        // has, for the `$arr['key'] === null` idiom instead of an
        // `is_*()` predicate.
        let pair = match (narrowing_key(l), narrowing_key(r)) {
            (Some(key), _) => Some((key, l, self.infer_expr(r, locals))),
            (None, Some(key)) => Some((key, r, self.infer_expr(l, locals))),
            (None, None) => None,
        };
        let Some((key, narrowed_expr, target_ty)) = pair else {
            return;
        };
        if target_ty == Type::Mixed {
            return;
        }
        let current = self.current_narrowed_type(narrowed_expr, &key, locals);
        let narrowed = if matches_when_true {
            narrow_type_to(&current, &target_ty)
        } else {
            narrow_type_excluding(&current, &target_ty)
        };
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
                self.registry.classes.contains_key(class) && !has_member(class)
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
                locals.insert(key, narrow_type_to(&current, target));
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
            "assertIsArray" => narrow_to(locals, 0, &Type::Array(Box::new(Type::Mixed))),
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
}

/// Phase 32 Finding F: the expression `current_narrowed_type`'s own
/// fallback should actually infer from, when no prior narrowing entry
/// exists yet. `narrowing_key`'s own unwrapping (`Grouping`, `Assign`,
/// the literal-`null`-coalesce case below) picks a *key* based on the
/// real underlying target, but inferring the type straight from the
/// original (still-wrapped) expression is only safe for some of those
/// shapes: `Grouping` is transparent (`infer_expr` already sees straight
/// through it), and `Assign` is deliberately *not* unwrapped here -
/// `infer_expr` on the whole `Assign` already gives exactly the
/// right-hand side's own type, which is what ends up stored under the
/// left-hand side too, so unwrapping it further would infer from the
/// *pre-assignment* value instead. A `??`-wrapped expression is
/// genuinely different: `infer_expr` on the whole coalesce already
/// strips `null` via the coalesce's own semantics before an `is_int`/
/// `is_string`/etc. predicate gets a chance to narrow anything from the
/// *real* left-hand type - inferring from the real target directly is
/// what lets `is_int($x['k'] ?? null)` see `$x['k']`'s own true type
/// (`int|null`), not the coalesce result's already-narrower one.
fn narrowing_infer_target(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Grouping(inner) => narrowing_infer_target(inner),
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
            if matches!(&rhs.kind, ExprKind::Literal(Literal::Null)) =>
        {
            narrowing_infer_target(lhs)
        }
        _ => e,
    }
}

/// A stable string key for the small set of expressions this checker can
/// narrow: a bare local variable, a property-access chain rooted in one
/// (`$a->b->c`), or a literal-key array-index chain rooted in one
/// (`$a['k1']['k2']`, `$a->b['k']`, any mix of the two). Lets `locals`
/// (otherwise keyed by plain variable name) also carry narrowed
/// property-chain/array-index types, reusing the exact same map rather
/// than a second, parallel one - safe because every synthetic key this
/// produces contains a character (`-` from `->`, or `[`/`]`) that can
/// never appear in a real PHP variable name (see `is_real_variable_key`,
/// the discriminator this invariant exists to support), so the key
/// spaces can't collide.
///
/// Without the `PropertyAccess` case, `apply_narrowing`'s `instanceof`
/// handling could only ever narrow a bare variable - `$arg->value
/// instanceof Variable && $arg->value->name` (an extremely common
/// AST-visitor idiom - narrow a property, then read a property only the
/// narrowed-to subtype has) had no way to remember that narrowing at
/// all, so the second half was checked against `$arg->value`'s
/// un-narrowed declared type. Found via real false positives once
/// undefined-property-on-read detection started actually exercising
/// this: nikic/php-parser AST-visitor code in this project's own
/// `tools/` does exactly this pattern.
///
/// Phase 22's own follow-on: the identical gap for a literal array-index
/// chain instead of a property chain - `is_string($lang_info
/// ['jquery_code'])` never narrowed `$lang_info['jquery_code']` at all
/// (every narrowing call site below required a bare `ExprKind::Variable`
/// directly, bypassing this function entirely, until Phase 22 changed
/// them to go through `narrowing_key` too), so a later read of the same
/// expression saw its un-narrowed, wider declared/inferred type - a real
/// corpus example (`Template::__construct`) fed that wider type into a
/// call whose declared parameter type no longer matched. A *dynamic*
/// key (`$a[$k]`) can't be narrowed this way - the same key at another
/// point in the code isn't guaranteed to be the same runtime index - so
/// only a literal key participates, matching `literal_string_key`'s own
/// existing use for the analogous shape-growing case in
/// `bind_assignment_target`.
pub(crate) fn narrowing_key(e: &Expr) -> Option<String> {
    match &e.kind {
        // No prefix - `locals` is keyed by bare variable name everywhere
        // else in this checker (`ExprKind::Variable(name) => locals.get(name)`),
        // and this has to produce the exact same key for a bare variable
        // to stay a drop-in replacement at the Instanceof-narrowing call
        // site.
        ExprKind::Variable(name) => Some(name.clone()),
        ExprKind::PropertyAccess(base, prop, _) => {
            narrowing_key(base).map(|base_key| format!("{base_key}->{prop}"))
        }
        ExprKind::Index(base, Some(idx)) => {
            let base_key = narrowing_key(base)?;
            match literal_string_key(idx) {
                Some(key) => Some(format!("{base_key}[{key}]")),
                // Phase 33 Finding 5: `is_string($this->mirror[$key])`
                // where `$key` is a variable, not a literal - a `$`-
                // prefixed marker distinguishes "keyed by this variable's
                // current value" from a literal key's own text (which
                // `is_real_variable_key` already treats as synthetic
                // either way, so this can't be mistaken for a real bare-
                // variable key). Only sound alongside real invalidation
                // of this entry whenever `subscript_name` is reassigned -
                // see `purge_stale_narrowing_for`, wired in everywhere a
                // bare variable's value can change.
                None => match &idx.kind {
                    ExprKind::Variable(subscript_name) => {
                        Some(format!("{base_key}[${subscript_name}]"))
                    }
                    _ => None,
                },
            }
        }
        // Phase 23: `is_string($result = expr)` - an assignment nested
        // inside the narrowed expression itself, the common "assign-then-
        // check" idiom. An assignment's own runtime value is exactly
        // what ends up stored under its left-hand side, so whatever gets
        // checked here is equally true of `lhs`'s own key afterward -
        // narrowing key can borrow the underlying variable/property/index
        // key without needing its own dedicated storage.
        ExprKind::Assign(lhs, _) => narrowing_key(lhs),
        // Phase 26: `($file = readdir($dir)) !== false` - the
        // "assign-then-check" idiom above, but wrapped in parens, which
        // real PHP *requires* here (`=` binds looser than `!==`, so the
        // parens aren't stylistic - `$file = readdir($dir) !== false`
        // would parse as `$file = (readdir($dir) !== false)` instead).
        // Without this, `Assign`'s own arm above never even got a chance
        // to run for the single most common shape it exists to cover.
        ExprKind::Grouping(inner) => narrowing_key(inner),
        // Phase 32 Finding F: `is_int($decoded['status'] ?? null)` - a
        // common defensive-coding idiom (json_decode results,
        // `$_SERVER`/`$_GET` reads, ...). Scoped to a literal `null`
        // fallback specifically, not any coalesce: `is_int($x['k'] ??
        // null)` being true can only happen if `$x['k']` itself was
        // already `int` (the fallback `null` would fail `is_int`
        // otherwise, so `null` specifically can never be the reason the
        // predicate passed) - but `is_int($x['k'] ?? 0)` gives no such
        // guarantee, since `$x['k']` being absent/`null` would still make
        // the whole expression `is_int`-true via the fallback `0`,
        // wrongly narrowing `$x['k']` itself away from a real possibility
        // (`null`) it could still hold.
        ExprKind::Binary(BinOp::Coalesce, lhs, rhs)
            if matches!(&rhs.kind, ExprKind::Literal(Literal::Null)) =>
        {
            narrowing_key(lhs)
        }
        _ => None,
    }
}

/// Whether `key` (as found in a `Locals` map) could be a real PHP
/// variable name, as opposed to one of `narrowing_key`'s own synthetic
/// property-chain/array-index keys - real PHP variable names are
/// restricted to `[A-Za-z_][A-Za-z0-9_]*`, so any other character (`-`
/// from `->`, `[`, `]`, ...) proves a key is synthetic, regardless of
/// which of `narrowing_key`'s own formats produced it. A shared,
/// character-class-based check rather than hunting for one specific
/// separator (`contains('-')`, say) - robust against `narrowing_key`
/// gaining a third synthetic shape later without every caller needing
/// its own update to keep excluding it correctly.
pub(crate) fn is_real_variable_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Recognizes `cond` as testing whether a `preg_match` call found a
/// match - bare (`if (preg_match(...))`) or compared against a literal
/// `0`/`1` in a way that unambiguously distinguishes "matched" from "not
/// matched". Deliberately excludes `!== false`/`=== false`: `preg_match`'s
/// return is `int|false`, and `false` doesn't distinguish a real "no
/// match" (`0`) from an actual error - where real PHP leaves `$matches`
/// completely *unchanged* from before the call (confirmed directly
/// against a live interpreter: a malformed pattern with a pre-existing
/// `$m` left `$m` untouched), a third state this project has no way to
/// represent (it isn't "the matched shape" or "empty," it's "whatever it
/// was before this statement") - the same imprecision real PHPStan's own
/// extensions accept, not something this closes either.
///
/// Only **strict** `===`/`!==` against exactly 0 or 1 are recognized -
/// PHP's *loose* `==`/`!=` would make `preg_match(...) == 0` ambiguous
/// between "no match" (`0`) and "error" (`false`, which loosely equals
/// `0` - confirmed directly: `false == 0` is `true` in PHP). `== 1`/`!= 1`
/// happen to still be safe even loosely (`false == 1` is `false`), but a
/// real grep of every `preg_match(_all)?(...)` comparison across this
/// project's own verification corpus found zero uses of loose `==`/`!=`
/// at all - every real comparison is already strict - so dropping loose
/// comparison recognition entirely costs no real coverage.
///
/// Scoped to plain `preg_match` (not `preg_match_all`, which needs none
/// of this - see Finding D), to no-`$flags` callers (mirrors this same
/// scope in `checker/expr.rs`'s own wiring), and to **positional**
/// arguments only - `Arg` carries its own `name: Option<String>` for a
/// named argument, and a bare positional destructure alone could
/// *misidentify* which expression is which under reordered named
/// arguments (`preg_match(subject: $s, pattern: $p, matches: $m)` would
/// otherwise bind `pattern` to what's actually `$s`), so any named
/// argument bails rather than guessing.
///
/// Returns `(pattern_arg, matches_arg, branch_true_means_matched)`.
fn preg_match_condition(cond: &Expr) -> Option<(&Expr, &Expr, bool)> {
    fn as_preg_match_args(e: &Expr) -> Option<&[Arg]> {
        match &e.kind {
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if name == "preg_match" => Some(args),
                _ => None,
            },
            ExprKind::Grouping(inner) => as_preg_match_args(inner),
            _ => None,
        }
    }
    // A real `fn`, not a closure - a closure can't express the
    // higher-ranked lifetime this needs (called once with `args`
    // borrowed from `cond` directly, once with `call_args` borrowed from
    // `l`/`r`; a closure infers one fixed lifetime that can't satisfy
    // both call sites).
    fn extract(args: &[Arg], matched: bool) -> Option<(&Expr, &Expr, bool)> {
        let [pattern, subject, matches] = args else {
            return None;
        };
        if pattern.name.is_some() || subject.name.is_some() || matches.name.is_some() {
            return None;
        }
        Some((&pattern.value, &matches.value, matched))
    }
    if let Some(args) = as_preg_match_args(cond) {
        return extract(args, true); // bare truthy: true branch = matched
    }
    let ExprKind::Binary(op, l, r) = &cond.kind else {
        return None;
    };
    let (call_args, lit_side) = match (as_preg_match_args(l), as_preg_match_args(r)) {
        (Some(a), _) => (a, r),
        (_, Some(a)) => (a, l),
        _ => return None,
    };
    let ExprKind::Literal(Literal::Int(lit)) = &lit_side.kind else {
        return None;
    };
    match (op, lit) {
        (BinOp::Identical, 1) => extract(call_args, true),
        (BinOp::NotIdentical, 1) => extract(call_args, false),
        (BinOp::Identical, 0) => extract(call_args, false),
        (BinOp::NotIdentical, 0) => extract(call_args, true),
        _ => None,
    }
}

/// Drops every `locals` entry that depended on `key`'s pre-reassignment
/// identity - either because `key` was itself the root a *deeper*
/// synthetic key was built from (`"key->prop"`, `"key[lit]"` - covers
/// both a bare-variable root and a full chain root, e.g. reassigning
/// `$foo->bar` invalidates a stale `"foo->bar->baz"`), or because `key`
/// (when it's a bare variable name) was used as a *variable subscript*
/// inside a different chain entirely (`"other[$key]"`, `narrowing_key`'s
/// own new key shape - never matches for a non-bare-variable `key`,
/// since a subscript marker only ever wraps a plain variable name).
///
/// Phase 33 Finding 5: without this, nothing ever invalidated a stale
/// composite-key entry when a variable it depends on changed - not just
/// for a variable-subscript key, but for *any* composite key at all
/// (reassigning `$foo` left a stale `"foo->bar"` sitting in `locals`
/// forever). Called from every real call site that can give a bare
/// variable, property chain, or index chain a new value.
///
/// Iterates and removes by key rather than a `retain` call - `Locals`'s
/// own doc comment (`checker.rs`) only documents `insert`/`remove`/
/// `entry`/`get`/`contains_key`/`iter` as verified-compatible with
/// `std::collections::HashMap` on the underlying `imbl` persistent map,
/// not `retain`.
pub(crate) fn purge_stale_narrowing_for(locals: &mut Locals, key: &str) {
    let base_prop = format!("{key}->");
    let base_idx = format!("{key}[");
    let subscript_marker = format!("[${key}]");
    let stale: Vec<String> = locals
        .iter()
        .filter(|(k, _)| {
            k.starts_with(&base_prop) || k.starts_with(&base_idx) || k.contains(&subscript_marker)
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        locals.remove(&k);
    }
}

pub(crate) fn narrowing_type_for_predicate(name: &str) -> Option<Type> {
    match name {
        "is_string" => Some(Type::String),
        "is_int" | "is_integer" | "is_long" => Some(Type::Int),
        "is_float" | "is_double" => Some(Type::Float),
        "is_bool" | "is_boolean" => Some(Type::Bool),
        "is_array" => Some(Type::Array(Box::new(Type::Mixed))),
        "is_null" => Some(Type::Null),
        "is_object" => Some(Type::Object("object".to_string(), Vec::new())),
        "is_callable" => Some(Type::Callable),
        // Phase 28, real shape: `proc_open()`/`fsockopen()` return
        // `resource|false`, guarded everywhere in this corpus with
        // `if (! is_resource($x)) { throw ...; }` before `fclose`/
        // `fwrite`/`fread`/`proc_close`/etc. - `resource` has no native
        // PHP type declaration, so it's represented the same
        // `Type::Object("resource", _)` way every other stub/checker
        // site in this project already does.
        "is_resource" => Some(Type::Object("resource".to_string(), Vec::new())),
        // `is_scalar($x)` is true for exactly `int|float|string|bool` -
        // unlike `is_numeric` (which this project deliberately doesn't
        // narrow at all, since real PHPStan's own `int|float|numeric-
        // string` result has no representation here and approximating
        // with `int|float` would wrongly exclude the equally-common
        // numeric-string case), `is_scalar`'s real set is exactly these
        // four concrete types with no such gap.
        "is_scalar" => Some(Type::Union(vec![
            Type::Int,
            Type::Float,
            Type::String,
            Type::Bool,
        ])),
        _ => None,
    }
}

/// If `cond` is an `Or`-tree (any depth, matching a left-associative
/// `A || B || C` chain) whose every leaf is a direct, single-argument
/// call to a name `narrowing_type_for_predicate` recognizes, all
/// narrowing the *identical* expression, returns that shared key, the
/// union of every leaf's own target type, and one representative leaf
/// expression (for `current_narrowed_type`'s own fallback, when the key
/// isn't already in `locals`) - `None` for anything else (a mixed bag of
/// keys, a non-simple-predicate leaf, `is_numeric`/`in_array`/
/// `array_key_exists`, none of which are `narrowing_type_for_predicate`
/// table entries, ...), the same conservative "give up" this project's
/// narrowing already falls back to everywhere else.
fn or_chain_narrowing_target(cond: &Expr) -> Option<(String, Type, &Expr)> {
    match &cond.kind {
        ExprKind::Binary(BinOp::Or, l, r) => {
            let (lk, lt, le) = or_chain_narrowing_target(l)?;
            let (rk, rt, _) = or_chain_narrowing_target(r)?;
            (lk == rk).then(|| (lk, union_types(lt, rt), le))
        }
        ExprKind::Grouping(inner) => or_chain_narrowing_target(inner),
        ExprKind::Call(callee, args) => {
            let (ExprKind::Name(name), [single_arg]) = (&callee.kind, args.as_slice()) else {
                return None;
            };
            let key = narrowing_key(&single_arg.value)?;
            let target_ty = narrowing_type_for_predicate(name)?;
            Some((key, target_ty, &single_arg.value))
        }
        _ => None,
    }
}

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
///
/// Phase 32 Finding L: a union member is kept when it's a *subtype* of
/// `target`, not just when it's structurally identical to it - exact
/// equality is the wrong tool for "is this the kind of thing being
/// narrowed": `is_array()`'s registered target is generic
/// `Type::Array(Mixed)`, so a specific-element `Array(String)` member
/// (from a docblock-refined param type) used to survive an `is_array()`
/// exclusion untouched, since `Array(String) != Array(Mixed)` even
/// though every value it describes plainly *is* an array. Deliberately
/// scoped to this `Union(members)` branch only, not the `_ => target.
/// clone()` fallback below it for a non-`Union` `original` - that
/// fallback already unconditionally trusts `target` regardless of
/// `original`'s own content, so there's nothing for `is_subtype` to
/// usefully change there.
pub(crate) fn narrow_type_to(original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| is_subtype(m, target))
                .cloned()
                .collect();
            match kept.len() {
                0 => target.clone(),
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => target.clone(),
    }
}

/// Narrows `original` by removing `excluded` along the branch where a check
/// for it failed - the "not null" half of a `=== null` guard clause, the
/// most common narrowing pattern in real code.
///
/// Phase 32 Finding L: a union member is dropped when it's a *subtype* of
/// `excluded`, not just when it's structurally identical to it - the same
/// reasoning as `narrow_type_to` above, mirrored for exclusion (`is_array
/// (...)`'s *false*-branch exclusion of `Array(Mixed)` used to leave a
/// specific-element `Array(String)` member behind, since it was never
/// `==` to the generic target it should have matched as "an array").
///
/// Deliberately **not** applied to the non-`Union` fallback below
/// (`other if other == excluded => Type::Never`): `is_subtype`'s own
/// int-to-float argument-coercion rule (`Type::Int` is a subtype of
/// `Type::Float`) would make `narrow_type_excluding(Type::Int,
/// Type::Float)` wrongly collapse a plain, correctly-known `int` to
/// `Type::Never` - the value genuinely is an int, and its coercibility to
/// `float` for parameter-passing purposes has nothing to do with whether
/// it should be excluded here. Every existing caller of this branch
/// (`isset`'s `Null` exclusion, and similar plain-`Null` exclusions
/// elsewhere) is unaffected either way, since no other `Type` variant is
/// ever a subtype of `Null`.
pub(crate) fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !is_subtype(m, excluded))
                .cloned()
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        other if other == excluded => Type::Never,
        other => other.clone(),
    }
}

/// Narrows `ty` by whether `key` is (`present = true`) or isn't
/// (`present = false`) provably a real offset of it -
/// `array_key_exists($key, $ty)`'s own true/false branches. An
/// `ArrayShape` member declaring `key` (required or optional) can always
/// make the call `true`; only a member declaring it *required* can be
/// proven to make the call `false` (an optional field's absence is
/// exactly what "optional" means, so its being false there is not
/// impossible for that member specifically). A member omitting `key`
/// altogether can never make the call `true`. Recurses through `Union`
/// the same way `narrow_type_to`/`narrow_type_excluding` do; anything
/// that isn't an `ArrayShape` (or a `Union` of them) is left untouched,
/// this project's usual "don't guess" precedent.
pub(crate) fn narrow_array_shape_by_key_presence(ty: &Type, key: &str, present: bool) -> Type {
    match ty {
        Type::ArrayShape(fields) => match fields.iter().find(|(k, _, _)| k == key) {
            Some((_, _, optional)) => {
                if !present && !*optional {
                    Type::Never
                } else {
                    ty.clone()
                }
            }
            None => {
                if present {
                    Type::Never
                } else {
                    ty.clone()
                }
            }
        },
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .map(|m| narrow_array_shape_by_key_presence(m, key, present))
                .filter(|m| *m != Type::Never)
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        _ => ty.clone(),
    }
}

/// Narrows `original` by excluding every falsy member this project can
/// represent (`Null`, `False`) - used for a bare truthiness check
/// (`if ($x)`) and `empty($x)`'s own truthy branch. Real PHP's full falsy
/// set also includes `0`/`0.0`/`""`/`"0"`/an empty array, but those aren't
/// independently representable as excludable members of a `Union` today.
pub(crate) fn narrow_type_excluding_falsy(original: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members
                .iter()
                .filter(|m| !matches!(m, Type::Null | Type::False))
                .cloned()
                .collect();
            match kept.len() {
                0 => Type::Never,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Null | Type::False => Type::Never,
        other => other.clone(),
    }
}
