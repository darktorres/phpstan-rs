//! Array-key legality: illegal key types for array-literal/dim-fetch access,
//! and duplicate/colliding array-literal keys.

use super::helpers::*;

// With a target php_version configured, additionally exclude a float
// key on PHP >= 8.1 and a null key on PHP >= 8.5.

#[test]
fn a_null_array_literal_key_is_flagged_on_php_8_5_or_later() {
    // A bare `null` literal, not `?string` - a *union* including a legal
    // member (`string`) would bail out of the whole check entirely (this
    // project's own "at least one member legal, stay silent" leniency),
    // so this needs a definite, non-union `Type::Null` to exercise the
    // new arm at all.
    let d = diagnostics_for_with(
        r#"<?php
            function test(): array {
                return [null => 1];
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.invalidKey"
            && x.message.contains("Invalid array key type null")),
        "got: {d:?}"
    );
}

// `offsetAccess.invalidOffset` applies the exact same legal-key-type
// check as `array.invalidKey` above, to a dim-fetch's own dimension
// expression (`$arr[$key]`) instead of a literal's key. A used generic
// template parameter must stay `Type::TemplateParam` here, not leak
// through as a bogus namespace-qualified class reference.

#[test]
fn an_unbound_generic_template_parameter_used_as_a_dim_fetch_key_is_never_flagged() {
    // A still-generic `T` reaching this check (not yet substituted with
    // a concrete type) must stay leniently unflagged, not misread as a
    // bogus, namespace-qualified class reference.
    // `invalid_array_key_description` has no arm for `Type::
    // TemplateParam` at all, so it falls to the safe "don't know"
    // catch-all.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T of int|string
             */
            class PriorityQueue {
                /** @var array<T, mixed> */
                private array $heap = [];

                /** @param T $priority */
                public function insert($priority, $value): void {
                    $this->heap[$priority] = $value;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_top_level_function_own_template_referenced_inside_an_inline_var_annotation_is_never_flagged() {
    // A top-level function's `@template Tk of array-key` referenced via
    // `/** @var Tk $key */` - no enclosing class at all, so the
    // class-level fix alone doesn't cover it; needs `Checker::
    // current_function_template_params` too. The `namespace` statement
    // is deliberate, not incidental: a global-namespace function's bare
    // name already equals its FQN, so a fixture without one wouldn't
    // actually exercise `current_function_template_params`'s fix
    // (`Registry::lookup_function` needs the real FQN,
    // `self.name_ctx.declared_name(...)` - a plain bare-name lookup
    // silently finds nothing for any namespaced function).
    //
    // `$keyFunc`'s own `(Closure(Tv): ?Tk)` type is copied verbatim from
    // the real signature too (not simplified to a bare `\Closure`) - `Tk`
    // is only ever referenced there, nested inside the closure's return-
    // type slot, never as a parameter's bare top-level type, so the
    // template-in-parameter scan must recurse into `Type::Callable` too,
    // not just an ordinary class's generic argument list.
    let d = diagnostics_for(
        r#"<?php
            namespace Psl\Dict;
            use Closure;
            /**
             * @template Tk of array-key
             * @template Tv
             * @param iterable<Tv> $values
             * @param (Closure(Tv): ?Tk) $keyFunc
             * @return array<Tk, list<Tv>>
             */
            function group_by(iterable $values, \Closure $keyFunc): array {
                $result = [];
                foreach ($values as $value) {
                    $key = $keyFunc($value);
                    /** @var Tk $key */
                    $result[$key] = $value;
                }
                return $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
