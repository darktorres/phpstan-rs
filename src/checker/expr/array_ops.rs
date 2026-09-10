use crate::ast::*;
use crate::checker::expr::{
    invalid_array_key_description, is_canonical_int_key, is_confidently_array,
};
use crate::checker::type_union::union_types;
use crate::checker::{Checker, Locals, TemplateBindings};
use crate::registry::FunctionSig;
use crate::token::Span;
use crate::types::{self, AccessoryKind, Type};

impl<'a> Checker<'a> {
    /// Phase 68: `DuplicateKeysInLiteralArraysRule` - a later literal key
    /// silently overwrites an earlier one at runtime (real PHP raises no
    /// warning either), almost always a real bug (a copy-pasted entry
    /// whose key was never updated) rather than an intentional
    /// "overwrite semantics" idiom. `infer_array_literal_type` just
    /// below already computes the exact same key-derivation/auto-index
    /// logic for its own overwrite-on-duplicate handling - this walks
    /// the same shape independently so it can report every duplicate
    /// found, rather than reusing that function's own discard-on-
    /// overwrite `Vec`. Bails on the *first* non-literal key, same
    /// "don't guess" leniency `infer_array_literal_type` already has -
    /// a computed key could coincide with any later key, so nothing
    /// past it can be soundly checked either.
    pub(crate) fn check_array_literal_duplicate_keys(&mut self, items: &[ArrayItem], label: &str) {
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
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => return,
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
                            label.to_string(),
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

    /// Finding 48: an array literal's own element types were never
    /// inferred precisely - every literal read as `Array(Mixed)`
    /// regardless of its actual contents, which silently defeated
    /// `strict_mixed_arrays` (Phase 11 Stage 5) for its own headline use
    /// case (checking a hand-written literal against a declared shape)
    /// and made any destructuring from an inline literal lose all
    /// per-position precision (Finding 42's own gap, from the same root
    /// cause). Builds a real `Type::ArrayShape`, keyed by each literal
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
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
        let mut next_index: i64 = 0;
        for item in items {
            let key = match &item.key {
                None => {
                    let k = next_index.to_string();
                    next_index += 1;
                    k
                }
                Some(k) => match &k.kind {
                    ExprKind::Literal(Literal::String(s)) => s.clone(),
                    ExprKind::Literal(Literal::Int(n)) => {
                        next_index = next_index.max(n + 1);
                        n.to_string()
                    }
                    _ => {
                        return Type::Array(
                            Box::new(Type::unknown_array_key()),
                            Box::new(Type::Mixed),
                        );
                    }
                },
            };
            let value_ty = self.infer_expr(&item.value, locals);
            // A later duplicate key overwrites an earlier one, matching
            // real PHP's own array-literal semantics.
            match fields.iter_mut().find(|(k, _, _)| *k == key) {
                Some(field) => field.1 = value_ty,
                None => fields.push((key, value_ty, false)),
            }
        }
        Type::ArrayShape(fields)
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
        let mut fields: Vec<(String, Type, bool)> = Vec::new();
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
                        Type::ArrayShape(inner_fields) => {
                            for (k, t, opt) in inner_fields {
                                Self::merge_shape_field(
                                    &mut fields,
                                    &mut next_index,
                                    k,
                                    t.clone(),
                                    *opt,
                                );
                            }
                        }
                        _ => degraded = true,
                    }
                }
                if degraded {
                    elem_types.push(Self::array_element_type(&value_ty).unwrap_or(Type::Mixed));
                    if !Self::is_list_like(&value_ty) {
                        list_safe = false;
                    }
                    if Self::is_known_non_empty_array(&value_ty) {
                        known_non_empty = true;
                    }
                }
            } else {
                if !degraded {
                    match &item.key {
                        None => {
                            let key = next_index.to_string();
                            next_index += 1;
                            match fields.iter_mut().find(|(fk, _, _)| *fk == key) {
                                Some(field) => field.1 = value_ty.clone(),
                                None => fields.push((key, value_ty.clone(), false)),
                            }
                        }
                        Some(k) => match &k.kind {
                            ExprKind::Literal(Literal::String(s)) => {
                                match fields.iter_mut().find(|(fk, _, _)| fk == s) {
                                    Some(field) => field.1 = value_ty.clone(),
                                    None => fields.push((s.clone(), value_ty.clone(), false)),
                                }
                            }
                            ExprKind::Literal(Literal::Int(n)) => {
                                next_index = next_index.max(n + 1);
                                let key = n.to_string();
                                match fields.iter_mut().find(|(fk, _, _)| *fk == key) {
                                    Some(field) => field.1 = value_ty.clone(),
                                    None => fields.push((key, value_ty.clone(), false)),
                                }
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
            return Type::ArrayShape(fields);
        }
        elem_types.extend(fields.into_iter().map(|(_, t, _)| t));
        let elem_ty = elem_types
            .into_iter()
            .reduce(union_types)
            .unwrap_or(Type::Mixed);
        let mut members = vec![Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(elem_ty),
        )];
        if list_safe {
            members.push(Type::Accessory(AccessoryKind::ArrayList));
        }
        if known_non_empty {
            members.push(Type::Accessory(AccessoryKind::NonEmptyArray));
        }
        if members.len() == 1 {
            members.pop().unwrap()
        } else {
            Type::Intersection(members)
        }
    }

    /// Shared by a spread array literal and `array_merge()`'s own bespoke
    /// special case - both apply the identical real-PHP merge rule
    /// verified via dumpType(): a source field's canonical int key gets
    /// renumbered sequentially into the target (`next_index`); any other
    /// key is kept and fully overwrites a same-named target field
    /// verbatim, its own optional flag included.
    pub(crate) fn merge_shape_field(
        fields: &mut Vec<(String, Type, bool)>,
        next_index: &mut i64,
        key: &str,
        ty: Type,
        optional: bool,
    ) {
        let resolved_key = if is_canonical_int_key(key) {
            let assigned = next_index.to_string();
            *next_index += 1;
            assigned
        } else {
            key.to_string()
        };
        match fields.iter_mut().find(|(fk, _, _)| *fk == resolved_key) {
            Some(field) => *field = (resolved_key, ty, optional),
            None => fields.push((resolved_key, ty, optional)),
        }
    }

    /// Whether `ty` is already provably int-key-sequential from zero - a
    /// real `list<T>`/`non-empty-list<T>`, or an `ArrayShape` whose own
    /// keys happen to be canonical and sequential. Shares the exact
    /// `is_subtype`-side rule for an `ArrayShape` against
    /// `Accessory::ArrayList` rather than re-deriving it.
    pub(crate) fn is_list_like(ty: &Type) -> bool {
        match ty {
            Type::Intersection(members) => {
                members.contains(&Type::Accessory(AccessoryKind::ArrayList))
            }
            Type::ArrayShape(fields) => fields
                .iter()
                .enumerate()
                .all(|(i, (k, _, _))| k == &i.to_string()),
            // Every branch of a control-flow-merged `Union` must itself
            // be list-like - an `ArrayShape([])` branch (the common
            // "0 iterations" case) already satisfies this vacuously via
            // the `ArrayShape` arm above.
            Type::Union(members) => members.iter().all(Self::is_list_like),
            _ => false,
        }
    }

    /// Whether `ty` is already provably non-empty - a real
    /// `non-empty-array<T>`/`non-empty-list<T>`, or a non-empty
    /// `ArrayShape` literal.
    pub(crate) fn is_known_non_empty_array(ty: &Type) -> bool {
        match ty {
            Type::Intersection(members) => {
                members.contains(&Type::Accessory(AccessoryKind::NonEmptyArray))
            }
            Type::ArrayShape(fields) => !fields.is_empty(),
            // Non-empty only if *every* branch is - a possibly-empty
            // branch (an `ArrayShape([])`, the common "0 iterations"
            // case) means the merged value isn't provably non-empty.
            Type::Union(members) => members.iter().all(Self::is_known_non_empty_array),
            _ => false,
        }
    }

    /// The mirror image of `is_known_non_empty_array` just above -
    /// whether `ty` is provably empty (an empty `ArrayShape` literal;
    /// nothing else in this project's own type system can prove zero
    /// elements, the same "don't guess" leniency `is_known_non_empty_
    /// array` already applies in the other direction).
    pub(crate) fn is_known_empty_array(ty: &Type) -> bool {
        match ty {
            Type::ArrayShape(fields) => fields.is_empty(),
            Type::Union(members) => members.iter().all(Self::is_known_empty_array),
            _ => false,
        }
    }

    /// Phase 71: `UselessFunctionReturnValueRule` -
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
        label: &str,
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
        self.report(
            label.to_string(),
            format!(
                "Return value of function {bare_name}() is always {useless_value} and the result is printed instead of being returned. Pass in true as parameter #2 $return to return the output instead."
            ),
            span,
            0,
            "function.uselessReturnValue",
        );
    }

    /// Phase 68: `ArrayFilterRule`, narrowed to just its `arrayFilter.
    /// empty` half (real PHP's own default no-callback "drop every falsy
    /// element" behavior on a provably-empty array). The other two real
    /// PHPStan sub-checks (`arrayFilter.same`/`arrayFilter.alwaysEmpty`,
    /// gated on the array's own *element* type being provably always-
    /// truthy/always-falsy) were dropped after corpus verification
    /// caught two live false positives - a dynamic-key index-assignment
    /// inside a loop's `if`-branch not correctly widening the tracked
    /// element type across the branch (`vendor/symfony/var-exporter/
    /// ProxyHelper.php`'s `$abstractProperties[$name] = new
    /// \ReflectionProperty(...)` vs `$abstractProperties[$name] =
    /// false;`, inferred as bare `array<false>` - the object-valued
    /// branch went missing entirely), and a `try { $x = ...; } finally {
    /// array_filter([..., $x]); }` shape not accounting for the
    /// possibility the assignment inside `try` never ran (Piwigo's own
    /// `tests/Integration/GroupServiceTest.php`, `$copyId = null;`
    /// reassigned only conditionally inside `try`, inferred as bare
    /// `GroupId` with the `null` branch dropped). Both are pre-existing
    /// gaps in this project's own variable-type tracking, not something
    /// to fix as part of this "cheap win" phase - `is_known_empty_array`
    /// below has no such exposure (purely syntactic, no flow-sensitive
    /// inference involved).
    pub(crate) fn check_array_filter_no_op(
        &mut self,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: Span,
    ) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !Self::is_array_like(&arr_ty) {
            return;
        }
        if Self::is_known_empty_array(&arr_ty) {
            self.report(
                label.to_string(),
                format!("call to array_filter() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayFilter.empty",
            );
        }
    }

    /// Phase 68: `ArrayValuesRule` - a no-op both when the input is
    /// provably empty and when it's already list-shaped (`is_list_like`,
    /// the same accessory-marker check this call's own return-type
    /// inference already relies on to know it *always* produces a fresh
    /// list regardless of the input's own shape).
    pub(crate) fn check_array_values_no_op(
        &mut self,
        args: &[Arg],
        locals: &Locals,
        label: &str,
        span: Span,
    ) {
        if args.len() != 1 {
            return;
        }
        let arr_ty = self.infer_expr(&args[0].value, locals);
        if !Self::is_array_like(&arr_ty) {
            return;
        }
        if Self::is_known_empty_array(&arr_ty) {
            self.report(
                label.to_string(),
                format!("call to array_values() with an empty array ({arr_ty}) has no effect"),
                span,
                0,
                "arrayValues.empty",
            );
        } else if Self::is_list_like(&arr_ty) {
            self.report(
                label.to_string(),
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
    pub(crate) fn infer_call_return_type(
        &self,
        sig: &FunctionSig,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> Type {
        let mut bindings: TemplateBindings = TemplateBindings::default();
        for (param, slot) in sig.params.iter().zip(resolved.iter()) {
            let Some(arg) = slot else { continue };
            let arg_ty = self.infer_expr(arg, locals);
            types::bind_template_param(&param.ty, &arg_ty, &mut bindings);
        }
        types::substitute_template(&sig.return_type, &bindings)
    }

    /// Phase 79: `Suit::from(1)`/`Suit::tryFrom(1)` called with a literal
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
            Some(case) => Type::EnumCase(class.to_string(), case.to_string()),
            None => fallback,
        }
    }

    /// Finding 48 made a literal array's own inferred type a real
    /// `Type::ArrayShape` instead of always `Array(Mixed)` - every one of
    /// Phase 12's own special cases below that used to pattern-match
    /// `Type::Array(elem_ty)` directly needs this instead, so
    /// `array_map(fn($x) => ..., [1, 2, 3])` (a literal, now a shape) keeps
    /// working the same way it already did for a plain `array<int>`-typed
    /// variable. A shape's own "effective element type" is the union of
    /// every field's type (`Mixed` for an empty literal - nothing to infer
    /// a narrower type from).
    /// Phase 50: real PHPStan's `InvalidKeyInArrayItemRule` - an array
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
    pub(crate) fn check_array_item_key_type(
        &mut self,
        key_expr: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let key_ty = self.infer_expr(key_expr, locals);
        if let Some(desc) = invalid_array_key_description(&key_ty) {
            self.report(
                label.to_string(),
                format!("Invalid array key type {desc}."),
                key_expr.span,
                3,
                "array.invalidKey",
            );
        }
    }

    /// Phase 50 (reattempt): real PHPStan's `InvalidKeyInArrayDimFetchRule`,
    /// `offsetAccess.invalidOffset` - confirmed directly against
    /// `vendor/phpstan-src` that this is genuinely the *same* check as
    /// `check_array_item_key_type` above (its own `AllowedArrayKeysTypes`
    /// legal-key-type set, same non-strict default), just applied to a
    /// dim-fetch's own dimension expression (`$arr[$key]`) instead of an
    /// array literal's key - not something that also needs to know
    /// `$arr`'s own *declared* key type (a different, separate concern
    /// this project doesn't model for dim-fetch at all - `ExprKind::
    /// Index`'s own value-type inference already gives up to `Mixed` for
    /// anything but a plain `Array`/`ArrayShape` base).
    ///
    /// A first attempt at this exact rule was built, corpus-verified,
    /// then reverted (see ROADMAP.md's own **Reverted** section) after
    /// finding +31 false positives, all traced to a used generic
    /// template parameter's type leaking through as a bogus, namespace-
    /// qualified `Object` reference instead of staying `Type::
    /// TemplateParam` (real example: `amphp/sync`'s own `PriorityQueue<T
    /// of int|string>`) - reusing this project's *own*, already-shared
    /// `invalid_array_key_description` verbatim sidesteps that: it has no
    /// arm for `Type::TemplateParam` at all, so it already falls to the
    /// safe "don't know, don't flag" catch-all for one, exactly the
    /// leniency a still-generic key type needs. Confirmed via a
    /// dedicated regression test mirroring the exact `PriorityQueue<T of
    /// int|string>` shape, and via a full corpus diff after landing (see
    /// ROADMAP.md's own Phase 50 entry).
    ///
    /// Fires on both a read (`$arr[$key]`) and a write (`$arr[$key] =
    /// ...`) - unlike `check_shape_key_access` (which deliberately skips
    /// the write side: a *missing* shape key isn't a bug on write, real
    /// PHP auto-vivifies it), this rule is about the key's own *type*,
    /// exactly as illegal on a write as a read regardless of whether the
    /// array grows - so it's called from both `check_expr`'s own
    /// `Index` arm and `check_assignment_target`'s.
    ///
    /// Only fires when `base`'s own inferred type is confidently
    /// array-shaped (a bare `Array`/`ArrayShape`, or a `Union` where
    /// *every* member is) - `Mixed`, an `ArrayAccess`-implementing
    /// object (offset access on one isn't modeled at all), or a `Union`
    /// mixing array and non-array members are all "don't know," matching
    /// this project's usual leniency rather than real PHPStan's own
    /// tri-state `isSuperTypeOf` grading (`reportMaybes`, off by
    /// default).
    pub(crate) fn check_dim_fetch_key_type(
        &mut self,
        base: &Expr,
        idx: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let base_ty = self.infer_expr(base, locals);
        if !is_confidently_array(&base_ty) {
            return;
        }
        let idx_ty = self.infer_expr(idx, locals);
        // A real, corpus-found residual of a pre-existing, disclosed gap
        // (`DocBlock::type_aliases`'s own doc comment: `@phpstan-import-
        // type Name from Class`, importing a `@phpstan-type` alias
        // declared on a *different* class, is "a distinct, separate tag
        // this project doesn't parse") - the unresolved alias name falls
        // through as a bogus, unrecognized `Object`, which this check
        // would otherwise misread as a confirmed-illegal key type instead
        // of "don't know" (real corpus shapes: `phpunit/php-code-coverage`'s
        // own `TestIndexType`, `php-cs-fixer`'s own `_ImportType`, both
        // `@phpstan-import-type`). `is_type_fully_known` is the same
        // "every `Object` reference is either a recognized pseudo-type or
        // a genuinely registered class" guard `check_generic_bounds`
        // already trusts for the identical reason - not new leniency
        // invented for this check.
        if !self.is_type_fully_known(&idx_ty) {
            return;
        }
        if let Some(desc) = invalid_array_key_description(&idx_ty) {
            self.report(
                label.to_string(),
                format!("Invalid array key type {desc}."),
                idx.span,
                3,
                "offsetAccess.invalidOffset",
            );
        }
    }

    pub(crate) fn array_element_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(_, elem) => Some((**elem).clone()),
            Type::ArrayShape(fields) => Some(
                fields
                    .iter()
                    .filter(|(name, _, _)| name != "...")
                    .map(|(_, t, _)| t.clone())
                    .reduce(union_types)
                    .unwrap_or(Type::Mixed),
            ),
            // Phase 14: a `list<T>`/`non-empty-array<T>`-typed value is a
            // real `Intersection([Array(T), Accessory(_)])` now, not
            // plain `Array(T)` - without this, every one of this
            // function's own callers (`array_map`/`array_reduce`/
            // `array_chunk`/`array_column`) silently stopped recognizing
            // such a value as an array at all.
            Type::Intersection(members) => members.iter().find_map(Self::array_element_type),
            // A round of Piwigo-corpus bucketing found this was missing
            // the single most common way a real variable's type actually
            // arrives here: a `Union` of array-like branches from a
            // control-flow merge (`$x = []; if (...) { foreach (...) {
            // $x[] = ...; } }` - the "0 iterations" branch stays
            // `array{}`, the "≥1" branch becomes a real list, and an
            // `if`/loop join leaves `$x` a `Union` of the two). A
            // genuinely empty `ArrayShape` member contributes no real
            // element-type constraint (it holds zero elements in that
            // branch) and is skipped rather than folded in as `Mixed`,
            // which would otherwise collapse the *whole* merged element
            // type down to `Mixed` the moment any branch was ever empty.
            // Only defined when every other member is itself array-like -
            // a `list<T>|null` union has no sensible single "element
            // type" to report.
            Type::Union(members) => {
                let mut elem_types = Vec::new();
                for m in members {
                    if matches!(m, Type::ArrayShape(fields) if fields.is_empty()) {
                        continue;
                    }
                    elem_types.push(Self::array_element_type(m)?);
                }
                Some(
                    elem_types
                        .into_iter()
                        .reduce(union_types)
                        .unwrap_or(Type::Mixed),
                )
            }
            _ => None,
        }
    }

    /// Phase 73: `array_element_type`'s own key-side counterpart -
    /// deliberately narrower, since only a bare `Type::Array` (or one
    /// wrapped in the `list<T>`/`non-empty-array<T>` `Intersection`
    /// shape) carries a real key type to report at all. An `ArrayShape`
    /// has no such field (each field's own key is a literal `String`,
    /// not a `Type`) - `is_list_like` already covers its one precise,
    /// commonly-useful key fact (are the keys exactly `0, 1, 2, ...`),
    /// so this returns `None` for one rather than trying to synthesize
    /// a real `Type::Int`/`Type::String`/`unknown_array_key()` from its
    /// field names, which none of this phase's own callers need.
    pub(crate) fn array_key_type(ty: &Type) -> Option<Type> {
        match ty {
            Type::Array(key, _) => Some((**key).clone()),
            Type::Intersection(members) => members.iter().find_map(Self::array_key_type),
            _ => None,
        }
    }

    /// Phase 14: whether `ty` is array-shaped at all - for sites that
    /// only need "is this even an array," not the unwrapped element
    /// type `array_element_type` above provides. Sees through a
    /// `list<T>`/`non-empty-array<T>`-typed value the same way, and
    /// through a `Union` of such values (the same control-flow-merge
    /// shape `array_element_type`'s own `Union` arm above handles).
    pub(crate) fn is_array_like(ty: &Type) -> bool {
        matches!(ty, Type::Array(_, _) | Type::ArrayShape(_))
            || matches!(ty, Type::Intersection(members)
                if members.iter().any(|m| matches!(m, Type::Array(_, _) | Type::ArrayShape(_))))
            || matches!(ty, Type::Union(members) if members.iter().all(Self::is_array_like))
    }
}
