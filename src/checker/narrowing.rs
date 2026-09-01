use crate::ast::*;
use crate::checker::{Checker, Locals, literal_string_key};
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Narrows `locals` in place based on `cond` being true (`branch_true`)
    /// or false, along one branch of an `if`/`while`. Only handles the
    /// common, safe patterns - equality against a known-type value
    /// (`$x === null`, `null !== $x`), `is_*()` guards, and `instanceof` -
    /// anything else leaves `locals` untouched rather than guess.
    pub(crate) fn apply_narrowing(&self, cond: &Expr, locals: &mut Locals, branch_true: bool) {
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
                    } else if let Some(target_ty) = narrowing_type_for_predicate(name) {
                        let current = self.current_narrowed_type(&single_arg.value, &key, locals);
                        let narrowed = if branch_true {
                            narrow_type_to(&current, &target_ty)
                        } else {
                            narrow_type_excluding(&current, &target_ty)
                        };
                        locals.insert(key, narrowed);
                    }
                }
            }
            ExprKind::Instanceof(inner, target) => {
                // The false branch would need a "not this class" type we
                // don't have a representation for, so it's left alone. A
                // dynamic target (`$x instanceof $className`) doesn't name
                // a concrete class to narrow to either.
                if branch_true
                    && let Some(key) = narrowing_key(inner)
                    && let InstanceofTarget::Name(class_name) = target
                {
                    locals.insert(key, self.resolve_name_expr(class_name));
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
            .unwrap_or_else(|| self.infer_expr(expr, locals))
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
            let key = literal_string_key(idx)?;
            narrowing_key(base).map(|base_key| format!("{base_key}[{key}]"))
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

/// Narrows `original` to `target` along the branch where a check for
/// `target` succeeded: if `original` is a union, keep only the member(s)
/// that match; otherwise just trust the more specific `target`.
pub(crate) fn narrow_type_to(original: &Type, target: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m == target).cloned().collect();
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
pub(crate) fn narrow_type_excluding(original: &Type, excluded: &Type) -> Type {
    match original {
        Type::Union(members) => {
            let kept: Vec<Type> = members.iter().filter(|m| *m != excluded).cloned().collect();
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
