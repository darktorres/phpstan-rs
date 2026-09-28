use crate::checker::expr::{
    invalid_array_key_description, is_canonical_int_key, is_confidently_array,
};
use crate::checker::{Checker, Locals, TemplateBindings};
use crate::registry::FunctionReflection;
use syntax::ast::*;
use syntax::token::Span;
use types::union_types;
use types::{self, AccessoryKind, Shape, ShapeField, ShapeKey, Type};

impl<'a> Checker<'a> {
    /// `DuplicateKeysInLiteralArraysRule` - a later literal key
    /// silently overwrites an earlier one at runtime (real PHP raises no
    /// warning either), almost always a real bug (a copy-pasted entry
    /// whose key was never updated) rather than an intentional
    /// "overwrite semantics" idiom. `infer_array_literal_type` just
    /// below already computes the exact same key-derivation/auto-index
    /// logic for its own overwrite-on-duplicate handling - this walks
    /// the same shape independently so it can report every duplicate
    /// found, rather than reusing that function's own discard-on-
    /// overwrite `Vec`. Bails on the *first* key it can't confidently
    /// resolve at all (via a bare AST literal, or `infer_expr` below),
    /// the same "don't guess" leniency `infer_array_literal_type` already
    /// has, since a computed key could coincide with any later key, so
    /// nothing past it can be soundly checked either.
    pub(crate) fn check_array_literal_duplicate_keys(
        &mut self,
        items: &[ArrayItem],
        locals: &Locals,
    ) {
        if items.iter().any(|item| item.spread) {
            return;
        }
        // Key text -> (first occurrence's span, how many times it's been
        // seen so far). Real PHP coerces a numeric-string key ("5") to
        // the same int key (5) it names - both an int key's own
        // `.to_string()` and its numeric-string twin are spelled
        // identically here, so comparing the plain text alone (no
        // separate int/string discriminator) already gets this right,
        // the same simplification `infer_array_literal_type` below
        // relies on for its own overwrite-on-duplicate handling.
        let mut seen: Vec<(String, (Span, u32))> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            let item_span = item.key.as_ref().map_or(item.value.span, |k| k.span);
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.to_string(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    // A negative int key literal parses as unary
                    // negation of a positive int literal, not a single
                    // negative `Literal::Int` token - without this arm,
                    // a literal negative key (`-5 => ...`) would fall
                    // into the catch-all below and silently abandon
                    // duplicate-key checking for the whole array
                    // literal, not just that one item.
                    ExprKind::Unary(UnaryOp::Neg, inner) => match &inner.kind {
                        ExprKind::Literal(Literal::Int(n)) => {
                            let n = -n;
                            next_index = next_index.max(n + 1);
                            n.to_string()
                        }
                        _ => return,
                    },
                    // Real PHPStan's own key resolution goes
                    // well beyond a bare AST literal (a class constant, a
                    // `defined()` constant, any statically-known-scalar
                    // expression) - routing through the same `infer_expr`
                    // every other key-typed consumer already uses gets
                    // all of that for free, canonicalizing PHP's own
                    // bool/null key coercion the same way a real key
                    // would (`true`/`false` become the int key `1`/`0`;
                    // `null` becomes the empty-string key `""`) - still
                    // deliberately bailing (not guessing) on anything
                    // that doesn't resolve this way either, e.g. a plain
                    // variable or an unresolvable constant.
                    _ => match self.infer_expr(k, locals) {
                        Type::IntLiteral(n) => {
                            next_index = next_index.max(n + 1);
                            n.to_string()
                        }
                        Type::StringLiteral(s) => s.to_string(),
                        Type::True => {
                            next_index = next_index.max(2);
                            "1".to_string()
                        }
                        Type::False => {
                            next_index = next_index.max(1);
                            "0".to_string()
                        }
                        Type::Null => String::new(),
                        _ => return,
                    },
                },
            };
            match seen.iter_mut().find(|(k, _)| *k == key) {
                Some((_, (first_span, count))) => {
                    *count += 1;
                    if *count == 2 {
                        // `var_export`-style display: bare for a
                        // canonical int key, single-quoted otherwise.
                        let display = if is_canonical_int_key(&key) {
                            key.clone()
                        } else {
                            format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"))
                        };
                        self.report(
                            format!("Array has duplicate key {display}."),
                            *first_span,
                            0,
                            "array.duplicateKey",
                        );
                    }
                }
                None => seen.push((key, (item_span, 1))),
            }
        }
    }

    /// An array literal's own element types must be inferred precisely:
    /// reading every literal as `Array(Mixed)` regardless of its
    /// actual contents would silently defeat `strict_mixed_arrays` for
    /// its own headline use case (checking a hand-written literal
    /// against a declared shape) and make any destructuring from an
    /// inline literal lose all per-position precision. Builds a real
    /// `Type::ArrayShape`, keyed by each literal
    /// key (or, for a positional element, PHP's own real auto-index
    /// rule: one past the highest explicit integer key seen so far, not
    /// just a running count - `[5 => 'a', 'b']`'s `'b'` really is key
    /// `6` in real PHP). A spread element (`...$other`) or a computed,
    /// non-literal key both defeat precise inference for the *whole*
    /// literal - real PHP inserts a variable number of the spread
    /// value's own elements at that point, shifting every subsequent
    /// positional key by an amount this checker has no way to know
    /// statically, and a non-literal key has no fixed name to key a
    /// field by at all - degrading to the honest `Array(Mixed)` for
    /// those specific cases, matching the status quo everywhere else.
    pub(crate) fn infer_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        if items.iter().any(|item| item.spread) {
            return self.infer_spread_array_literal_type(items, locals);
        }
        let mut shape = Shape::sealed(Vec::new());
        let mut next_index: i64 = 0;
        for item in items {
            let key = match &item.key {
                None => {
                    let k = ShapeKey::Int(next_index);
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => {
                        let key = ShapeKey::from_name(&s.to_str_lossy());
                        if let ShapeKey::Int(n) = key {
                            next_index = next_index.max(n + 1);
                        }
                        key
                    }
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        ShapeKey::Int(*n)
                    }
                    _ => {
                        return Type::array(Type::unknown_array_key(), Type::mixed());
                    }
                },
            };
            let value_ty = self.infer_expr(&item.value, locals);
            // A later duplicate key overwrites an earlier one, matching
            // real PHP's own array-literal semantics.
            shape.set(ShapeField::new(key, value_ty, false));
        }
        Type::array_shape(shape)
    }

    /// A spread item (`[...$a, ...$b]`) can be tracked precisely as long
    /// as every spread source's own exact fields are known (an
    /// `ArrayShape`) - real PHP's own array-unpacking semantics
    /// (verified via `dumpType()`): a spread `ArrayShape`'s canonical int
    /// keys get renumbered sequentially into the result, exactly the same
    /// as a plain positional item; its non-canonical (string, or a
    /// non-sequential int) keys are kept and overwrite verbatim, the same
    /// `is_canonical_int_key` rule `infer_array_literal_type`'s own
    /// int-key handling already applies. `UserInfoWithThemeName::toArray()`
    /// (`[...$this->userInfo->toArray(), 'theme_name' => $this->themeName]`)
    /// is exactly this shape - merging two known shapes into one, not
    /// combining unknown-length arrays.
    ///
    /// The moment any spread source's fields *aren't* known this way (a
    /// plain `array<T>`/`list<T>`), the whole literal degrades to the
    /// coarser "element type plus list/non-empty markers" level instead -
    /// and every field already collected before that point degrades with
    /// it, keeping only its value type, not its specific key. Verified via
    /// `dumpType()`: `['x' => 1, ...$plainArray]` on a plain `array<int>`
    /// dumps as `non-empty-array<int>`, not a shape retaining `'x'`.
    /// Spreading two `list<T>`s stays a `list<T>`; a plain `array<T>`
    /// source (list-ness unproven) degrades list-ness for the whole
    /// result, matching `[...$list, ...$plainArray] -> array<T>` (not
    /// `list<T>`) exactly. A plain, unkeyed element always guarantees the
    /// result is non-empty; a spread source only does if it's itself
    /// provably non-empty.
    fn infer_spread_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        let mut shape = Shape::sealed(Vec::new());
        let mut next_index: i64 = 0;
        let mut degraded = false;
        let mut elem_types: Vec<Type> = Vec::new();
        let mut list_safe = true;
        let mut known_non_empty = false;

        for item in items {
            let value_ty = self.infer_expr(&item.value, locals);
            if item.spread {
                if !degraded {
                    match &value_ty {
                        Type::ArrayShape(inner) => {
                            for f in &inner.fields {
                                Self::merge_shape_field(
                                    &mut shape,
                                    &mut next_index,
                                    &f.key,
                                    f.ty.clone(),
                                    f.optional,
                                );
                            }
                        }
                        _ => degraded = true,
                    }
                }
                if degraded {
                    elem_types.push(value_ty.array_value_type().unwrap_or(Type::mixed()));
                    // `is_list_like` alone (the original check) wrongly
                    // treats an *unmarked* int-keyed source
                    // (`array<int, int>`, list-ness never proven, only
                    // never disproven) as breaking list-ness - real PHP
                    // spread semantics always renumber a source's own
                    // *integer* keys sequentially, so only a possible
                    // *string* key actually does (confirmed via
                    // `dumpType()` against real PHPStan). Extending the
                    // check to the source's own key type, rather than
                    // replacing `is_list_like` outright, matters: a
                    // real `list<T>`-tagged source's own boxed key type is
                    // always the vestigial `unknown_array_key()`
                    // (`int|string`) regardless of its real, accessory-
                    // proven list-ness (list-ness is signaled only by the
                    // `ArrayList` accessory in this project's own
                    // representation, never by the boxed key type) - a
                    // key-type-only check would wrongly call such a
                    // source *not* list-safe.
                    if !Self::is_list_safe_array_merge_argument(&value_ty) {
                        list_safe = false;
                    }
                    if value_ty.is_non_empty_array().yes() {
                        known_non_empty = true;
                    }
                }
            } else {
                if !degraded {
                    match &item.key {
                        None => {
                            let key = ShapeKey::Int(next_index);
                            next_index += 1;
                            shape.set(ShapeField::new(key, value_ty.clone(), false));
                        }
                        Some(k) => match &k.kind {
                            ExprKind::Literal(Literal::String(s)) => {
                                let key = ShapeKey::from_name(&s.to_str_lossy());
                                if let ShapeKey::Int(n) = key {
                                    next_index = next_index.max(n + 1);
                                }
                                shape.set(ShapeField::new(key, value_ty.clone(), false));
                            }
                            ExprKind::Literal(Literal::Int(n)) => {
                                next_index = next_index.max(n + 1);
                                shape.set(ShapeField::new(
                                    ShapeKey::Int(*n),
                                    value_ty.clone(),
                                    false,
                                ));
                            }
                            _ => degraded = true,
                        },
                    }
                }
                elem_types.push(value_ty);
                known_non_empty = true;
                if item.key.is_some() {
                    list_safe = false;
                }
            }
        }

        if !degraded {
            return Type::array_shape(shape);
        }
        elem_types.extend(shape.fields.into_iter().map(|f| f.ty));
        let elem_ty = elem_types
            .into_iter()
            .reduce(union_types)
            .unwrap_or(Type::mixed());
        let mut members = vec![Type::array(Type::unknown_array_key(), elem_ty)];
        if list_safe {
            members.push(Type::accessory(AccessoryKind::ArrayList));
        }
        if known_non_empty {
            members.push(Type::accessory(AccessoryKind::NonEmptyArray));
        }
        if members.len() == 1 {
            members.pop().unwrap()
        } else {
            Type::intersection_of(members)
        }
    }

    /// Shared by a spread array literal and `array_merge()`'s own bespoke
    /// special case - both apply the identical real-PHP merge rule
    /// verified via dumpType(): a source field's canonical int key gets
    /// renumbered sequentially into the target (`next_index`); any other
    /// key is kept and fully overwrites a same-named target field
    /// verbatim, its own optional flag included.
    pub(crate) fn merge_shape_field(
        shape: &mut Shape,
        next_index: &mut i64,
        key: &ShapeKey,
        ty: Type,
        optional: bool,
    ) {
        let resolved_key = match key {
            ShapeKey::Int(_) => {
                let assigned = ShapeKey::Int(*next_index);
                *next_index += 1;
                assigned
            }
            ShapeKey::Str(_) => key.clone(),
        };
        shape.set(ShapeField::new(resolved_key, ty, optional));
    }

    /// `UselessFunctionReturnValueRule` -
    /// `var_export`/`print_r`/`highlight_string` all share the same
    /// `bool $return = false` last parameter: printing directly and
    /// returning a useless `null`/`true` when it's omitted or literally
    /// `false`, or returning the real string instead of printing when
    /// it's `true`. Gated on `!is_bare_statement` at the one call site
    /// (see `Checker::in_bare_statement_position`'s own doc comment) -
    /// a bare `var_export($x);` statement genuinely wants the print
    /// side effect and legitimately discards the useless return value,
    /// the normal way to use any of these three; only flagged once
    /// something else tries to *use* that return value instead
    /// (assigned, returned, passed as an argument, ...).
    pub(crate) fn check_useless_return_value_call(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &Locals,
        span: Span,
    ) {
        let bare_name = name.rsplit('\\').next().unwrap_or(name);
        let useless_value = if bare_name.eq_ignore_ascii_case("var_export") {
            "null"
        } else if bare_name.eq_ignore_ascii_case("print_r")
            || bare_name.eq_ignore_ascii_case("highlight_string")
        {
            "true"
        } else {
            return;
        };
        // Only fires when the print-vs-return flag is definitely absent
        // or definitely `false` - a genuinely unknown (non-literal,
        // non-narrowed) second argument stays silent, "don't guess": it
        // really could be `true` at runtime.
        let is_useless = match args.len() {
            0 => return,
            1 => true,
            _ => self.infer_expr(&args[1].value, locals) == Type::False,
        };
        if !is_useless {
            return;
        }
        self.report(format!(
                "Return value of function {bare_name}() is always {useless_value} and the result is printed instead of being returned. Pass in true as parameter #2 $return to return the output instead."
            ),
            span,
            0,
            "function.uselessReturnValue",
        );
    }

    /// `ArrayFilterRule` - `arrayFilter.empty`
    /// (real PHP's own default no-callback "drop every falsy element"
    /// behavior on a provably-empty array) and the other two real PHPStan
    /// sub-checks (`arrayFilter.same`/`arrayFilter.alwaysEmpty`, gated on
    /// the array's own element type being provably always-truthy/
    /// always-falsy) are all unconditional, matching real PHPStan's own
    /// unconditional level-5 default for this exact check (no separate
    /// opt-in `featureToggles.*` parameter exists for it there either).
    /// This relies on accurate variable-type-tracking through loops and
    /// try/finally to avoid false positives - a dynamic-key
    /// index-assignment inside a loop's `if`-branch must correctly widen
    /// the tracked element type across the branch, and a `try { $x =
    /// ...; } finally { array_filter([..., $x]); }` shape must account
    /// for the possibility the assignment inside `try` never ran.
    pub(crate) fn check_array_filter_no_op(&mut self, args: &[Arg], locals: &Locals, span: Span) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !arr_ty.is_array().yes() {
            return;
        }
        if arr_ty.is_empty_array().yes() {
            self.report(
                format!("call to array_filter() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayFilter.empty",
            );
            return;
        }
        let Some(element_ty) = arr_ty.array_value_type() else {
            return;
        };
        match array_filter_element_falsy_verdict(&element_ty) {
            Some(false) => {
                self.report(format!(
                        "call to array_filter() with an array of only non-falsy values ({arr_ty}) has no effect"
                    ),
                    span,
                    0,
                    "arrayFilter.same",
                );
            }
            Some(true) => {
                self.report(format!(
                        "call to array_filter() with an array of only falsy values ({arr_ty}) always returns an empty array"
                    ),
                    span,
                    0,
                    "arrayFilter.alwaysEmpty",
                );
            }
            None => {}
        }
    }

    /// `ArrayValuesRule` - a no-op both when the input is
    /// provably empty and when it's already list-shaped (`is_list_like`,
    /// the same accessory-marker check this call's own return-type
    /// inference already relies on to know it *always* produces a fresh
    /// list regardless of the input's own shape).
    pub(crate) fn check_array_values_no_op(&mut self, args: &[Arg], locals: &Locals, span: Span) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !arr_ty.is_array().yes() {
            return;
        }
        if arr_ty.is_empty_array().yes() {
            self.report(
                format!("call to array_values() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayValues.empty",
            );
        } else if arr_ty.is_list().yes() {
            self.report(
                format!(
                    "call to array_values() on {arr_ty}, which is already a list, has no effect"
                ),
                span,
                0,
                "arrayValues.list",
            );
        }
    }

    /// A generic function's return type may mention its own `@template`
    /// parameters (`TemplateParam`) - this binds each one against the
    /// actual argument at this call site (`identity(5)` binds `T` to
    /// `int`) and substitutes the result into the return type. A signature
    /// with no templates at all is untouched (the common case, cheap).
    ///
    /// Also resolves a `Type::Conditional` in the
    /// (already template-substituted) return type - `param_bindings`
    /// (parameter *name* -> actual argument type) is a deliberately
    /// separate map from `bindings` above (template *name* -> bound type),
    /// built from the same per-param loop; substituting templates first
    /// matters, since a conditional's own branches may themselves mention
    /// one (`@return ($x is T ? T : null)`).
    pub(crate) fn infer_call_return_type(
        &self,
        sig: &FunctionReflection,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> Type {
        let mut bindings: TemplateBindings = TemplateBindings::default();
        let mut param_bindings: rustc_hash::FxHashMap<String, Type> =
            rustc_hash::FxHashMap::default();
        for (param, slot) in sig.params().iter().zip(resolved.iter()) {
            let Some(arg) = slot else {
                // An omitted argument's conditional-return subject is its
                // default value, as in real PHPStan.
                if let Some(default_ty) = &param.default_ty() {
                    param_bindings.insert(param.name().to_string(), default_ty.clone());
                }
                continue;
            };
            let arg_ty = self.infer_expr(arg, locals);
            // Same literal-class-string promotion as
            // the `ExprKind::New` constructor-binding arm - only for
            // template binding, never for `param_bindings` (which feeds
            // `resolve_conditional`'s `$x is T` checks and must see the
            // argument's real, unpromoted type).
            let promoted = types::promote_class_string_literals(param.ty(), &arg_ty, &|s| {
                self.registry.lookup_class(s).map(|(c, _)| c.to_string())
            });
            types::bind_template_param(param.ty(), &promoted, &mut bindings);
            param_bindings.insert(param.name().to_string(), arg_ty);
        }
        // The call's own templates (scoped to this function or method,
        // not to a class) that no argument inferred resolve to their
        // bounds; a class template left unbound here belongs to an
        // erased receiver and stays `Mixed`.
        let substituted =
            types::substitute_template_resolving_to_bounds(sig.return_type(), &bindings, &|tp| {
                tp.scope
                    .as_deref()
                    .is_some_and(|scope| !self.registry.classes.contains_key(scope))
            });
        types::resolve_conditional(&substituted, &param_bindings)
    }

    /// `Suit::from(1)`/`Suit::tryFrom(1)` called with a literal
    /// argument that provably matches one specific case's own backing
    /// value narrows past the synthesized `self`/`?self` signature
    /// straight to that case's own singleton type -
    /// `Registry::enum_case_for_value` is a no-op (returns `None`) for
    /// every non-enum class and every enum whose cases this checker
    /// couldn't prove literal values for, so this never fires outside a
    /// real backed-enum `from`/`tryFrom` call. A literal that matches
    /// *no* known case is deliberately left alone (returns `fallback`
    /// unchanged) rather than guessing `never`/`null` - real PHP's own
    /// `from()` runtime-error path isn't modeled, and this project
    /// doesn't have every case's value proven literal often enough to
    /// safely conclude "none of them match." Shared by both the
    /// mutating `check_expr` and the pure `infer_expr` `StaticCall` arms
    /// so they stay in sync.
    pub(crate) fn narrow_enum_from_call(
        &self,
        class: &str,
        method: &str,
        args: &[Arg],
        fallback: Type,
        locals: &Locals,
    ) -> Type {
        if (method != "from" && method != "tryFrom") || args.len() != 1 {
            return fallback;
        }
        let arg_ty = self.infer_expr(&args[0].value, locals);
        match self.registry.enum_case_for_value(class, &arg_ty) {
            Some(case) => Type::enum_case(class.into(), case.into()),
            None => fallback,
        }
    }

    /// A literal array's own inferred type is a real
    /// `Type::ArrayShape` instead of always `Array(Mixed)`, so a caller
    /// that pattern-matches `Type::Array(elem_ty)` directly needs
    /// `array_element_type` instead, so
    /// `array_map(fn($x) => ..., [1, 2, 3])` (a literal shape) keeps
    /// working the same way it already does for a plain `array<int>`-typed
    /// variable. A shape's own "effective element type" is the union of
    /// every field's type (`Mixed` for an empty literal - nothing to infer
    /// a narrower type from).
    /// Real PHPStan's `InvalidKeyInArrayItemRule` - an array
    /// literal's own key expression (`[$key => $value]`) must be a type
    /// PHP itself accepts as an array key at runtime (`int`/`string`,
    /// plus `bool`/`float`/`null`, which PHP silently coerces) - anything
    /// else (an array, an object, ...) is a hard `TypeError` at runtime,
    /// not a static-analysis nicety. Mirrors real PHPStan's own
    /// `AllowedArrayKeysTypes::getType()` default (non-strict) allowed
    /// set; unlike real PHPStan, has no tri-state "possibly invalid"
    /// case - `invalid_array_key_description` only ever answers when
    /// every part of the key's own type is confidently illegal, the same
    /// "don't guess" leniency this project applies everywhere else, so a
    /// `Mixed`/unresolved/partially-legal key is silently allowed rather
    /// than flagged.
    pub(crate) fn check_array_item_key_type(&mut self, key_expr: &Expr, locals: &Locals) {
        let key_ty = self.infer_expr(key_expr, locals);
        if let Some(desc) = invalid_array_key_description(&key_ty, self.registry.php_version_max())
        {
            self.report(
                format!("Invalid array key type {desc}."),
                key_expr.span,
                3,
                "array.invalidKey",
            );
        }
        if self.settings.extras.check_array_key_cast
            && let Some((from, to)) = array_key_cast_description(key_expr, &key_ty)
        {
            self.report(
                format!("Key {from} will be cast to {to} in the array."),
                key_expr.span,
                4,
                "array.keyCast",
            );
        }
    }

    /// Real PHPStan's `InvalidKeyInArrayDimFetchRule`,
    /// `offsetAccess.invalidOffset` - genuinely the same check as
    /// `check_array_item_key_type` above (its own `AllowedArrayKeysTypes`
    /// legal-key-type set, same non-strict default), just applied to a
    /// dim-fetch's own dimension expression (`$arr[$key]`) instead of an
    /// array literal's key - not something that also needs to know
    /// `$arr`'s own declared key type (a different, separate concern
    /// this project doesn't model for dim-fetch at all - `ExprKind::
    /// Index`'s own value-type inference already gives up to `Mixed` for
    /// anything but a plain `Array`/`ArrayShape` base).
    ///
    /// A used generic template parameter's type could otherwise leak
    /// through as a bogus, namespace-qualified `Object` reference instead
    /// of staying `Type::TemplateParam` (e.g. `amphp/sync`'s own
    /// `PriorityQueue<T of int|string>`) - reusing this project's own,
    /// already-shared `invalid_array_key_description` verbatim sidesteps
    /// that: it has no arm for `Type::TemplateParam` at all, so it
    /// already falls to the safe "don't know, don't flag" catch-all for
    /// one, exactly the leniency a still-generic key type needs.
    ///
    /// Fires on both a read (`$arr[$key]`) and a write (`$arr[$key] =
    /// ...`) - unlike `check_offset_existence` (which deliberately skips
    /// the write side: a *missing* shape key isn't a bug on write, real
    /// PHP auto-vivifies it), this rule is about the key's own *type*,
    /// exactly as illegal on a write as a read regardless of whether the
    /// array grows - so it's called from both `check_expr`'s own
    /// `Index` arm and `check_assignment_target`'s.
    ///
    /// Only fires when `base`'s own inferred type is confidently
    /// array-shaped (a bare `Array`/`ArrayShape`, or a `Union` where
    /// every member is) - `Mixed`, an `ArrayAccess`-implementing
    /// object (offset access on one isn't modeled at all), or a `Union`
    /// mixing array and non-array members are all "don't know," matching
    /// this project's usual leniency rather than real PHPStan's own
    /// tri-state `isSuperTypeOf` grading (`reportMaybes`, off by
    /// default).
    pub(crate) fn check_dim_fetch_key_type(&mut self, base: &Expr, idx: &Expr, locals: &Locals) {
        let base_ty = self.infer_expr(base, locals);
        if !is_confidently_array(&base_ty) {
            return;
        }
        let idx_ty = self.infer_expr(idx, locals);
        // An unresolved `@phpstan-import-type Name from Class` alias
        // (`DocBlock::type_aliases`'s own doc comment: a distinct,
        // separate tag this project doesn't parse) falls through as a
        // bogus, unrecognized `Object`, which this check would otherwise
        // misread as a confirmed-illegal key type instead of "don't
        // know". `is_type_fully_known` is the same "every `Object`
        // reference is either a recognized pseudo-type or a genuinely
        // registered class" guard `check_generic_bounds` already trusts
        // for the identical reason.
        if !self.is_type_fully_known(&idx_ty) {
            return;
        }
        if let Some(desc) = invalid_array_key_description(&idx_ty, self.registry.php_version_max())
        {
            self.report(
                format!("Invalid array key type {desc}."),
                idx.span,
                3,
                "offsetAccess.invalidOffset",
            );
        }
        if self.settings.extras.check_array_key_cast
            && let Some((from, to)) = array_key_cast_description(idx, &idx_ty)
        {
            self.report(
                format!("Key {from} will be cast to {to} in the array access."),
                idx.span,
                4,
                "offsetAccess.keyCast",
            );
        }
    }

    /// Whether a key type could genuinely include a `String` member - the
    /// only thing that actually breaks list-ness for a real PHP operation
    /// that renumbers integer keys sequentially and discards their
    /// original values (a spread array literal, and `array_merge()`,
    /// which applies the identical renumbering rule to its own plain-
    /// array arguments - shared by both `infer_spread_array_literal_
    /// type`'s own degraded branch and `array_merge`'s below).
    /// Conservative (`true`) for anything not provably int-only, matching
    /// this project's "don't know, don't guess" leniency elsewhere.
    pub(crate) fn key_type_could_be_string(ty: &Type) -> bool {
        match ty {
            Type::String | Type::StringLiteral(_) => true,
            Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => false,
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(Self::key_type_could_be_string)
            }
            _ => true,
        }
    }

    /// `key_type_could_be_string`'s own inverse-shaped sibling - whether a
    /// key type could genuinely include an `Int` member. `array_merge`'s
    /// own coarse-fallback key-type computation uses this (alongside
    /// `key_type_could_be_string`) to decide whether an argument's real
    /// keys could ever get real PHP's int-key-renumbering treatment,
    /// rather than always widening the merged key type to `int|string`
    /// regardless of what the arguments actually prove.
    pub(crate) fn key_type_could_be_int(ty: &Type) -> bool {
        match ty {
            Type::Int | Type::IntLiteral(_) | Type::IntRange(_, _) => true,
            Type::String | Type::StringLiteral(_) => false,
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(Self::key_type_could_be_int)
            }
            _ => true,
        }
    }

    /// `array_merge`'s own per-argument list-safety check - the same
    /// "already proven list, or at least provably not string-keyed" OR
    /// `infer_spread_array_literal_type`'s degraded branch already uses,
    /// factored out since both apply the identical real-PHP renumbering
    /// rule to their own plain-array arguments/spread sources.
    pub(crate) fn is_list_safe_array_merge_argument(ty: &Type) -> bool {
        ty.is_list().yes()
            || ty
                .array_key_type()
                .is_some_and(|k| !Self::key_type_could_be_string(&k))
    }
}

/// Real PHP's own array-key cast (mirrors `ConstantScalarType::
/// toArrayKey()`): a float truncates to int, `true`/`false` become
/// `1`/`0`, `null` becomes `''`, and a canonical-integer string
/// (`canonical_int_key`) becomes that int - every other constant scalar
/// (a plain int, or a non-canonical string like `"08"`/`"+1"`) is
/// already its own array key and stays unchanged. Returns `Some((from,
/// to))` (both already `describe`-formatted) only when a cast actually
/// happens; `None` for a non-constant-scalar type (`Mixed`, a bare
/// `Int`/`String`, ...) too, matching `check_array_item_key_type`'s own
/// "don't guess" leniency.
///
/// A bare float literal expression infers to `Type::Float`, not a
/// `FloatLiteral`, everywhere else in this checker (nothing else needs
/// the literal value kept around) - checked directly off the AST here
/// instead, the one shape this function needs it for.
fn array_key_cast_description(expr: &Expr, ty: &Type) -> Option<(String, String)> {
    if let ExprKind::Literal(Literal::Float(f)) = &unwrap_grouping(expr).kind {
        return Some((f.to_string(), (*f as i64).to_string()));
    }
    match ty {
        Type::FloatLiteral(f) => Some((f.to_string(), (*f as i64).to_string())),
        Type::True => Some(("true".to_string(), "1".to_string())),
        Type::False => Some(("false".to_string(), "0".to_string())),
        Type::Null => Some(("null".to_string(), "''".to_string())),
        Type::StringLiteral(s) => {
            types::canonical_int_key(&s.to_str_lossy()).map(|n| (format!("'{s}'"), n.to_string()))
        }
        _ => None,
    }
}

/// Real PHP truthiness classification for `array_filter()`'s
/// own `arrayFilter.same`/`arrayFilter.alwaysEmpty` checks - `Some(true)`
/// when `ty` is confidently *always* falsy, `Some(false)` when it's
/// confidently *never* falsy, `None` when it's ambiguous (real PHPStan's
/// own tri-state "maybe" - neither `isSuperTypeOf(...)::yes()` nor
/// `::no()`). Deliberately *not* built on `narrow_type_excluding_falsy`
/// (`checker/narrowing/array_shape.rs`) - that function is designed for
/// condition-narrowing, where a bare, unrefined `Type::Int`/`Float`/`Bool`
/// staying unchanged after "narrowing" is a harmless missed-precision
/// opportunity; reusing that same identity-preserving behavior *here*
/// (comparing narrowed-vs-original to decide "never falsy") would be
/// unsound - a bare `int`-typed element genuinely can be exactly `0`,
/// so it must classify as ambiguous, not "never falsy." This project has
/// no `FloatLiteral` variant at all, so a bare `Type::Float` is always
/// ambiguous too (never provably `0.0` or provably nonzero).
fn array_filter_element_falsy_verdict(ty: &Type) -> Option<bool> {
    match ty {
        Type::Null | Type::False => Some(true),
        Type::True => Some(false),
        Type::IntLiteral(n) => Some(*n == 0),
        Type::StringLiteral(s) => Some(s.is_empty() || s == "0"),
        Type::ArrayShape(shape) => {
            if shape.is_empty_array() {
                Some(true)
            } else if !shape.fields.is_empty() {
                Some(false)
            } else {
                // Only an unsealed tail - real size unknown.
                None
            }
        }
        // An object/enum-case instance, a resource, or a `class-string`,
        // is always truthy in real PHP regardless of its own contents.
        Type::Object(..)
        | Type::ObjectWithoutClass
        | Type::Resource { .. }
        | Type::EnumCase(..)
        | Type::ClassString(..)
        | Type::ClassKindString(_) => Some(false),
        // A `non-falsy-string` (never `""`/`"0"`) or `non-empty-array`
        // (never the empty array) accessory intersection is confidently
        // never falsy, whatever its own unrefined base type is. `list`
        // (`AccessoryKind::ArrayList`) deliberately does not qualify on
        // its own: `list<T>` only promises sequential int keys, never
        // non-emptiness - `[]` is a perfectly real, empty `list<T>`
        // (`types::parse::parse_type_string`'s own `"list"` arm adds
        // `ArrayList` alone, `NonEmptyArray` only for `non-empty-list`)
        // - so `list<T>`'s own truthiness stays genuinely ambiguous,
        // matching real PHPStan exactly.
        Type::Intersection(members) => members
            .iter()
            .any(|m| {
                matches!(
                    m,
                    Type::Accessory(AccessoryKind::NonFalsyString | AccessoryKind::NonEmptyArray)
                )
            })
            .then_some(false),
        Type::IntRange(lo, hi) => {
            if *lo == Some(0) && *hi == Some(0) {
                Some(true)
            } else if lo.is_some_and(|l| l > 0) || hi.is_some_and(|h| h < 0) {
                Some(false)
            } else {
                None
            }
        }
        Type::Union(members) => {
            let verdicts: Option<Vec<bool>> = members
                .iter()
                .map(array_filter_element_falsy_verdict)
                .collect();
            let verdicts = verdicts?;
            if verdicts.iter().all(|v| *v) {
                Some(true)
            } else if verdicts.iter().all(|v| !*v) {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}
