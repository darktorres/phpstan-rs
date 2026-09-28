use crate::checker::Checker;
use crate::registry::Registry;
use syntax::token::Span;
use types::Type;

/// Whether `ty` is a real PHP generator-compatible return type,
/// mirroring real PHP's own engine-level check exactly, which only needs one member of a
/// union to be `Traversable`-compatible, not all of them (`int|Generator`
/// is genuinely accepted by real PHP, however misleading, and `iterable`
/// itself is `array|Traversable` - the `array` half alone would
/// otherwise wrongly sink the whole check). `Mixed` (no useful
/// information either way) is treated as always compatible, matching
/// this project's usual "don't guess against yourself" leniency for an
/// unresolved/undeclared type.
pub(crate) fn type_allows_generator_return(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Mixed(_) => true,
        Type::Object(name, _) => registry.is_subclass_of(name, "Traversable"),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .any(|m| type_allows_generator_return(m, registry)),
        _ => false,
    }
}

/// Whether a generator's own falls-off-the-end/bare-`return;`
/// completeness check should still fire, and if so, against what -
/// `None` means still fully exempt (no declared return type at all, or
/// one with no useful `TReturn` precision: `Mixed`/`Void`, the
/// overwhelming common case absent `@return Generator<K,V,S,TReturn>`
/// docblock detail), matching real PHPStan's own `if ($returnType->
/// isVoid()->yes()) { return []; }` early-out in `MissingReturnRule`.
/// `Some(t)` means report against `t` (the TReturn type) instead of the
/// outer `Generator<...>` declaration itself - a real `return;`/fall-
/// through never produces an actual `Generator` object, so checking
/// against the outer type directly would always mismatch.
pub(crate) fn generator_missing_return_type(
    declared: Option<&Type>,
    registry: &Registry,
) -> Option<Type> {
    match declared.map(|t| generator_template_param(t, registry, "TReturn"))? {
        Type::Mixed(_) | Type::Void => None,
        treturn => Some(treturn),
    }
}

/// `ty`'s own binding for one of `Generator<TKey, TValue,
/// TSend, TReturn>`'s four template params (`param` is `"TSend"` or
/// `"TReturn"`) - `Mixed` when `ty` isn't `Generator`-shaped at all (a
/// bare `iterable`/`Iterator<K,V>` declaration, an unparameterized bare
/// `Generator`, or no declared type at all), matching real PHPStan's own
/// `YieldHandler`/`YieldFromHandler` fallback exactly. No new `Type`
/// variant needed - `Generator`'s own stub `ClassReflection` already declares
/// its four real `@template`/`@template-covariant` tags (`vendor/
/// phpstorm-stubs/standard/_types.php`), so `class_template_bindings`
/// (already built for ordinary generic-class method-call resolution)
/// does the whole job unchanged.
pub(crate) fn generator_template_param(ty: &Type, registry: &Registry, param: &str) -> Type {
    match ty {
        Type::Object(name, args) => registry
            .class_template_bindings(name, args, "Generator")
            .get(param)
            .cloned()
            .unwrap_or(Type::mixed()),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .find_map(|m| match generator_template_param(m, registry, param) {
                Type::Mixed(_) => None,
                found => Some(found),
            })
            .unwrap_or(Type::mixed()),
        _ => Type::mixed(),
    }
}

impl<'a> Checker<'a> {
    /// `YieldFromTypeRule`'s equivalent - up to five
    /// diagnostics on one `yield from $inner` node, called right after
    /// `inner_ty` (the delegated generator/iterable's own type) is
    /// computed. All four type-comparison diagnostics are skipped when
    /// the *enclosing* declared return type is `Mixed`-shaped (no real
    /// `@return Generator<...>` docblock precision at all - the
    /// overwhelmingly common case), matching real PHPStan's own
    /// `if ($returnType instanceof MixedType) { return []; }` early-out.
    pub(crate) fn check_yield_from_compatibility(
        &mut self,
        inner_ty: &Type,
        result_ty: &Type,
        span: Span,
        is_bare_statement: bool,
    ) {
        if yield_from_arg_not_iterable(inner_ty, self.registry) {
            self.report(format!("Argument of an invalid type {inner_ty} supplied for yield from, only iterables are supported."),
                span,
                0,
                "generator.nonIterable",
            );
        }
        let Some(enclosing) = self.current_declared_return_type.clone() else {
            return;
        };
        if enclosing == Type::Mixed(false) {
            return;
        }
        // Real PHPStan's own `generator.void` check: the `yield from`
        // expression's own result (the delegated generator's TReturn) is
        // `void` but consumed in a non-statement position.
        if *result_ty == Type::Void && !is_bare_statement {
            self.report("Result of yield from is used but the delegated generator never returns anything, it's return type is void.".to_string(),
                span,
                0,
                "generator.void",
            );
        }
        // The remaining three comparisons only have anything real to
        // compare against for a delegated *Generator*-shaped value -
        // a plain `array`/other `Traversable` doesn't carry `TSend`, and
        // its own key/value types aren't available here yet either.
        // Don't guess past what's known.
        let Type::Object(name, args) = inner_ty else {
            return;
        };
        let inner_bindings = self
            .registry
            .class_template_bindings(name, args, "Generator");
        if inner_bindings.is_empty() {
            return;
        }
        let inner_key = inner_bindings.get("TKey").cloned().unwrap_or(Type::mixed());
        let inner_value = inner_bindings
            .get("TYield")
            .cloned()
            .unwrap_or(Type::mixed());
        let inner_send = inner_bindings
            .get("TSend")
            .cloned()
            .unwrap_or(Type::mixed());
        let enclosing_key = generator_template_param(&enclosing, self.registry, "TKey");
        let enclosing_value = generator_template_param(&enclosing, self.registry, "TYield");
        let enclosing_send = generator_template_param(&enclosing, self.registry, "TSend");
        if enclosing_key != Type::Mixed(false)
            && inner_key != Type::Mixed(false)
            && !self.rule_level_accepts(&enclosing_key, &inner_key).yes()
        {
            self.report(format!(
                    "Key type {inner_key} of yielded generator is not assignable to key type {enclosing_key} of enclosing generator."
                ),
                span,
                0,
                "generator.keyType",
            );
        }
        if enclosing_value != Type::Mixed(false)
            && inner_value != Type::Mixed(false)
            && !self
                .rule_level_accepts(&enclosing_value, &inner_value)
                .yes()
        {
            self.report(format!(
                    "Value type {inner_value} of yielded generator is not assignable to value type {enclosing_value} of enclosing generator."
                ),
                span,
                0,
                "generator.valueType",
            );
        }
        // Contravariant direction, deliberately reversed vs. key/value:
        // the delegated generator's own TSend must accept everything the
        // enclosing one does (a caller `->send()`ing into the outer
        // generator delegates that same value into the inner one).
        if enclosing_send != Type::Mixed(false)
            && inner_send != Type::Mixed(false)
            && !self.accepts(&inner_send, &enclosing_send).yes()
        {
            self.report(format!(
                    "Send type {enclosing_send} of enclosing generator is not assignable to send type {inner_send} of yielded generator."
                ),
                span,
                0,
                "generator.sendType",
            );
        }
    }
}

/// Whether `ty` is definitely not iterable (`array|
/// Traversable`) - mirrors `thrown_value_not_throwable`'s own recursive
/// shape (`checker/expr/member_resolution.rs`), but with `Union`
/// deliberately flipped to "every member must fail" (not first-match) to
/// stay on the false-negative side: `int|Generator` really is iterable
/// half the time, so only report when *no* member could possibly be.
fn yield_from_arg_not_iterable(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Mixed(_) => false,
        Type::Array(_, _) | Type::ArrayShape(_) => false,
        Type::Object(class, _) => !registry.is_subclass_of(class, "Traversable"),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .all(|m| yield_from_arg_not_iterable(m, registry)),
        _ => true,
    }
}
