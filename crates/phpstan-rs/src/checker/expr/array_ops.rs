use crate::checker::expr::invalid_array_key_description;
use crate::checker::{Checker, Locals, TemplateBindings};
use crate::registry::FunctionReflection;
use syntax::ast::*;
use syntax::span::Span;
use types::UnionAll as _;

use types::{
    self, AccessoryKind, AppendKey, NextArrayIndex, Shape, ShapeField, ShapeKey, TrinaryLogic, Type,
};

impl<'a> Checker<'a> {
    /// `DuplicateKeysInLiteralArraysRule`: a later key in an array literal
    /// silently overwrites an earlier one, almost always a copy-paste bug.
    /// Keys resolve as `infer_array_literal_type` resolves them
    /// (`array_literal_key`), so `1`, `'1'` and `true` are the same key;
    /// the check stops at the first key that could be any of several.
    pub(crate) fn check_array_literal_duplicate_keys(
        &mut self,
        items: &[ArrayItem],
        locals: &Locals,
    ) {
        if items.iter().any(|item| item.spread) {
            return;
        }
        // Key -> (first occurrence's span, how many times it's been seen
        // so far).
        let mut seen: Vec<(ShapeKey, (Span, u32))> = Vec::new();
        let mut next_index = self.registry.next_array_index();
        for item in items {
            let item_span = item.key.as_ref().map_or(item.value.span, |k| k.span);
            let key = match &item.key {
                // Appending past `PHP_INT_MAX` throws, so nothing after it
                // is an element; a key the PHP versions disagree on may
                // coincide with a later one or not.
                None => match next_index.take() {
                    AppendKey::Key(n) => ShapeKey::Int(n),
                    AppendKey::Unknown | AppendKey::Throws => return,
                },
                // A key that could be any of several coincides with a
                // later one or not; nothing past it can be checked.
                Some(k) => match self.array_literal_key(k, locals) {
                    Ok(key) => {
                        if let ShapeKey::Int(n) = key {
                            next_index.observe(n);
                        }
                        key
                    }
                    Err(_) => return,
                },
            };
            match seen.iter_mut().find(|(k, _)| *k == key) {
                Some((_, (first_span, count))) => {
                    *count += 1;
                    if *count == 2 {
                        // Bare for an int key, quoted as PHP writes it
                        // otherwise.
                        let display = match &key {
                            ShapeKey::Int(n) => n.to_string(),
                            ShapeKey::Str(text) => types::quote_php_string(text),
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

    /// An array literal's type: a shape keyed by each item's key (a
    /// positional item's is the next index, one past the highest int key
    /// so far) while every key is known, and a non-empty general array of
    /// every key and value once one isn't. A spread item goes through
    /// `infer_spread_array_literal_type`.
    pub(crate) fn infer_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        if items.iter().any(|item| item.spread) {
            return self.infer_spread_array_literal_type(items, locals);
        }
        let mut shape = Shape::sealed(Vec::new());
        let mut next_index = self.registry.next_array_index();
        // Once a key isn't known, every key and value so far and after.
        let mut general: Option<(Vec<Type>, Vec<Type>)> = None;
        for item in items {
            let key = match &item.key {
                // Appending past `PHP_INT_MAX` throws: the literal has no
                // value.
                None => match next_index.take() {
                    AppendKey::Key(n) => Ok(ShapeKey::Int(n)),
                    AppendKey::Unknown => Err(Type::int()),
                    AppendKey::Throws if general.is_some() => Err(Type::int()),
                    AppendKey::Throws => return Type::never(),
                },
                Some(k) => self.array_literal_key(k, locals),
            };
            let value_ty = self.infer_expr(&item.value, locals);
            match (key, &mut general) {
                (Ok(key), None) => {
                    if let ShapeKey::Int(n) = key {
                        next_index.observe(n);
                    }
                    // A later duplicate key overwrites an earlier one.
                    shape.set(ShapeField::new(key, value_ty, false));
                }
                (key, general) => {
                    let (keys, values) = general.get_or_insert_with(|| {
                        shape
                            .fields()
                            .iter()
                            .map(|f| (f.key.to_type(), f.ty.clone()))
                            .unzip()
                    });
                    keys.push(key.map_or_else(|ty| ty, |key| key.to_type()));
                    values.push(value_ty);
                }
            }
        }
        match general {
            None => Type::array_shape(shape),
            Some((keys, values)) => general_array_literal_type(&self.relation(), keys, values),
        }
    }

    /// An array literal item's key: the one PHP stores it under when the
    /// key expression's type pins it down (`-1`, `'5'` as `5`, `true` as
    /// `1`, a constant), otherwise the key type it may be.
    pub(crate) fn array_literal_key(&self, key: &Expr, locals: &Locals) -> Result<ShapeKey, Type> {
        let key_ty = self.infer_expr(key, locals).to_array_key();
        ShapeKey::from_type(&key_ty).ok_or(key_ty)
    }

    /// An array literal with spread items (`[...$a, 'k' => $v]`). While
    /// every spread source is a known shape and every key is known, the
    /// result is a shape: a spread renumbers integer keys to the next
    /// index and keeps string keys, overwriting, as `array_merge` does.
    /// After that it is a general array of every key and value: a list
    /// while every source is list-safe and no item has a key, non-empty
    /// once a plain item or a non-empty source appears.
    fn infer_spread_array_literal_type(&self, items: &[ArrayItem], locals: &Locals) -> Type {
        let mut shape = Shape::sealed(Vec::new());
        let mut next_index = self.registry.next_array_index();
        let mut degraded = false;
        let mut key_types: Vec<Type> = Vec::new();
        let mut elem_types: Vec<Type> = Vec::new();
        let mut list_safe = true;
        let mut known_non_empty = false;

        for item in items {
            let value_ty = self.infer_expr(&item.value, locals);
            if item.spread {
                if !degraded {
                    if let [inner] = value_ty.constant_arrays()[..] {
                        for f in inner.fields() {
                            match Self::merge_shape_field(
                                &mut shape,
                                &mut next_index,
                                &f.key,
                                f.ty.clone(),
                                f.optional,
                            ) {
                                Ok(()) => {}
                                Err(AppendKey::Throws) => return Type::never(),
                                Err(_) => degraded = true,
                            }
                        }
                    } else {
                        degraded = true;
                    }
                }
                if degraded {
                    elem_types.push(value_ty.array_value_type().unwrap_or(Type::mixed()));
                    // Spreading renumbers integer keys and keeps string
                    // ones.
                    let source_key = value_ty
                        .array_key_type()
                        .unwrap_or_else(Type::unknown_array_key);
                    key_types.push(match source_key.is_int() {
                        TrinaryLogic::Yes => Type::int(),
                        TrinaryLogic::No => source_key,
                        TrinaryLogic::Maybe => self.relation().union(Type::int(), source_key),
                    });
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
                let key = match &item.key {
                    None => match next_index.take() {
                        AppendKey::Key(n) => Ok(ShapeKey::Int(n)),
                        AppendKey::Throws if !degraded => return Type::never(),
                        AppendKey::Unknown | AppendKey::Throws => Err(Type::int()),
                    },
                    Some(k) => self.array_literal_key(k, locals),
                };
                match key {
                    Ok(key) if !degraded => {
                        if let ShapeKey::Int(n) = key {
                            next_index.observe(n);
                        }
                        shape.set(ShapeField::new(key, value_ty.clone(), false));
                    }
                    key => {
                        degraded = true;
                        key_types.push(match key {
                            Ok(ShapeKey::Int(_)) if item.key.is_none() => Type::int(),
                            Ok(key) => key.to_type(),
                            Err(ty) => ty,
                        });
                        elem_types.push(value_ty.clone());
                    }
                }
                known_non_empty = true;
                if item.key.is_some() {
                    list_safe = false;
                }
            }
        }

        if !degraded {
            return Type::array_shape(shape);
        }
        for field in shape.into_fields() {
            key_types.push(field.key.to_type());
            elem_types.push(field.ty);
        }
        let key_ty = if list_safe {
            Type::unknown_array_key()
        } else {
            key_types
                .into_iter()
                .union_all_with(&self.relation())
                .unwrap_or_else(Type::unknown_array_key)
        };
        let elem_ty = elem_types
            .into_iter()
            .union_all_with(&self.relation())
            .unwrap_or(Type::mixed());
        let mut members = vec![Type::array(key_ty, elem_ty)];
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

    /// Merges one field into `shape` as spreading or `array_merge` does:
    /// an integer key is renumbered to the next index, a string key kept
    /// and overwriting, its optional flag included. `Err` with the next
    /// index when renumbering has no key: it throws, or the PHP versions
    /// disagree on it.
    pub(crate) fn merge_shape_field(
        shape: &mut Shape,
        next_index: &mut NextArrayIndex,
        key: &ShapeKey,
        ty: Type,
        optional: bool,
    ) -> Result<(), AppendKey> {
        let resolved_key = match key {
            ShapeKey::Int(_) => match next_index.take() {
                AppendKey::Key(n) => ShapeKey::Int(n),
                other => return Err(other),
            },
            ShapeKey::Str(_) => key.clone(),
        };
        shape.set(ShapeField::new(resolved_key, ty, optional));
        Ok(())
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
            _ => self.infer_expr(&args[1].value, locals).bool_literal_value() == Some(false),
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
        match element_ty
            .to_boolean()
            .bool_literal_value()
            .map(|truthy| !truthy)
        {
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
    /// Also resolves a conditional type in the
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
        // A `value-of<Enum>` the call's bindings made concrete needs the
        // enum's cases.
        self.registry
            .resolve_deferred_value_of_now(self.bound_call_return_type(sig, resolved, locals))
    }

    /// `infer_call_return_type` before class references are resolved.
    fn bound_call_return_type(
        &self,
        sig: &FunctionReflection,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> Type {
        // Without a template reference, a late reference or a conditional,
        // nothing a binding says changes the return type: none is made.
        let return_type = sig.return_type();
        if !return_type.needs_call_binding() {
            return return_type.clone();
        }
        let conditional = return_type.has_parameter_conditional();
        let (bindings, arguments) = self.call_bindings(sig, resolved, locals, conditional);
        // The call's own templates (scoped to this function or method,
        // not to a class) that no argument inferred resolve to their
        // bounds; a class template left unbound here belongs to an
        // erased receiver and stays `Mixed`.
        let substituted = self.substitute_call_templates(return_type, &bindings);
        // A parameter is found by its name, the last of that name as a
        // table keyed by name would hold it.
        let params = sig.params();
        let argument = |name: &str| {
            params
                .iter()
                .rposition(|p| p.name().as_str() == name)
                .and_then(|i| arguments.get(i))
                .and_then(Option::as_ref)
        };
        substituted.resolve_conditional(&argument, &self.relation())
    }

    /// `ty` with a call's template bindings substituted, its own templates
    /// that no argument bound resolving to their bounds.
    pub(crate) fn substitute_call_templates(&self, ty: &Type, bindings: &TemplateBindings) -> Type {
        let relation = self.relation();
        ty.substitute_templates_resolving_to_bounds(
            bindings,
            &|tp| {
                tp.scope
                    .as_deref()
                    .is_some_and(|scope| !self.registry.classes().contains_key(scope))
            },
            &relation,
        )
    }

    /// What a call binds `sig`'s templates to from its arguments
    /// (`resolved`, in parameter order).
    pub(crate) fn call_template_bindings(
        &self,
        sig: &FunctionReflection,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) -> TemplateBindings {
        self.call_bindings(sig, resolved, locals, false).0
    }

    /// What a call binds `sig`'s templates to from its arguments, and, when
    /// `with_arguments`, the type passed for each parameter (an omitted
    /// one's default) for the conditional types that name it.
    fn call_bindings(
        &self,
        sig: &FunctionReflection,
        resolved: &[Option<&Expr>],
        locals: &Locals,
        with_arguments: bool,
    ) -> (TemplateBindings, Vec<Option<Type>>) {
        let mut bindings: TemplateBindings = TemplateBindings::default();
        let mut arguments: Vec<Option<Type>> = Vec::new();
        for (param, slot) in sig.params().iter().zip(resolved.iter()) {
            let Some(arg) = slot else {
                // An omitted argument's conditional-return subject is its
                // default value, as in real PHPStan.
                if with_arguments {
                    arguments.push(param.default_ty().clone());
                }
                continue;
            };
            let arg_ty = self.infer_expr(arg, locals);
            // Same literal-class-string promotion as
            // the `ExprKind::New` constructor-binding arm - only for
            // template binding, never for the arguments (which feed
            // `resolve_conditional`'s `$x is T` checks and must see the
            // argument's real, unpromoted type).
            let promoted = param.ty().promote_class_string_literals(&arg_ty, &|s| {
                self.registry.lookup_class(s).map(|(c, _)| c.to_string())
            });
            self.relation()
                .infer_template_bindings(param.ty(), &promoted, &mut bindings);
            if with_arguments {
                arguments.push(Some(arg_ty));
            }
        }
        (bindings, arguments)
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
    /// array shape instead of always `Array(Mixed)`, so a caller
    /// that reads only a general array's value type needs
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
            && let Some((from, to)) = array_key_cast_description(&key_ty)
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
    /// of staying a template (e.g. `amphp/sync`'s own
    /// `PriorityQueue<T of int|string>`) - reusing this project's own,
    /// already-shared `invalid_array_key_description` verbatim sidesteps
    /// that: it has no arm for a template at all, so it
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
        if !base_ty.is_array().yes() {
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
            && let Some((from, to)) = array_key_cast_description(&idx_ty)
        {
            self.report(
                format!("Key {from} will be cast to {to} in the array access."),
                idx.span,
                4,
                "offsetAccess.keyCast",
            );
        }
    }

    /// `array_merge`'s own per-argument list-safety check - the same
    /// "already proven list, or at least provably not string-keyed" OR
    /// `infer_spread_array_literal_type`'s degraded branch already uses,
    /// factored out since both apply the identical real-PHP renumbering
    /// rule to their own plain-array arguments/spread sources.
    pub(crate) fn is_list_safe_array_merge_argument(ty: &Type) -> bool {
        ty.is_list().yes() || ty.array_key_type().is_some_and(|k| k.is_string().no())
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
fn array_key_cast_description(ty: &Type) -> Option<(String, String)> {
    if ty.finite_types().len() != 1 {
        return None;
    }
    let key = ty.to_array_key();
    (key != *ty).then(|| (ty.to_string(), key.to_string()))
}

/// An array literal with a key that isn't known: every key and value it
/// has, non-empty.
fn general_array_literal_type(
    relation: &types::Relation,
    keys: Vec<Type>,
    values: Vec<Type>,
) -> Type {
    let key = keys
        .into_iter()
        .union_all_with(relation)
        .unwrap_or_else(Type::unknown_array_key);
    let value = values
        .into_iter()
        .union_all_with(relation)
        .unwrap_or_else(Type::mixed);
    Type::intersection_of(vec![
        Type::array(key, value),
        Type::accessory(AccessoryKind::NonEmptyArray),
    ])
}
