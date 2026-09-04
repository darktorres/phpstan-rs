//! Doctrine- and stdlib-specific special-cased inference (stdClass, curl, json, etc).

use super::helpers::*;

// Phase 13 Finding A: getenv()'s real stub is unconditionally
// `array|string|false`, but a truly zero-argument call only ever
// returns `array`, and a call with a real, non-null `$name` only
// ever returns `string|false`.
#[test]
fn getenv_with_a_literal_name_narrows_to_string_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string|false {
                return getenv('X');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn getenv_with_no_arguments_narrows_to_a_plain_array() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return getenv();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn getenv_with_an_explicit_null_name_keeps_the_broad_type() {
    // A real, live PHP fact confirmed against real PHPStan too:
    // `getenv(null)` returns the full environment array, exactly
    // like the zero-argument call - it must NOT get the
    // plain-`array` answer, and must NOT narrow to `string|false`.
    let d = diagnostics_for(
        r#"<?php
            function f(): string|false {
                return getenv(null);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
}

#[test]
fn getenv_with_a_local_only_named_argument_keeps_the_broad_type() {
    // `getenv(local_only: true)` leaves `$name` at its own `null`
    // default (confirmed against real PHPStan's own dumpType()) -
    // `args.is_empty()` is false here, so this must not get the
    // plain-`array` answer either.
    let d = diagnostics_for(
        r#"<?php
            function f(): string|false {
                return getenv(local_only: true);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
}

#[test]
fn getenv_with_a_possibly_null_name_keeps_the_broad_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $name): string|false {
                return getenv($name);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "expected the broad type to still apply: {d:?}");
}

// A round of Piwigo-corpus bucketing found the native stub's own
// plain `string|false` was the single largest non-array-related
// false-positive source: real PHPStan's own dynamic-return-type
// extension always returns a real non-falsy-string on success
// (verified via dumpType()), letting the common `$jar = tempnam(...);
// if ($jar === false) { throw ...; }` idiom feed a
// non-empty-string-typed sink afterward.
#[test]
fn tempnam_on_success_narrows_to_a_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $path */
            function sink(string $path): void {}
            function f(): void {
                $jar = tempnam(sys_get_temp_dir(), 'x');
                if ($jar === false) {
                    throw new RuntimeException('x');
                }
                sink($jar);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Same family as tempnam() above: the native stub declares a bare
// array|false, but real PHPStan's own dynamic-return-type extension
// always returns a real list<string> on success, verified via
// dumpType(). Real Piwigo test code (StructuralTest's own
// findCallSitesInRootPhpFiles) narrows away false via `$paths !==
// false ? $paths : []` and relies on the result being a real
// list<string>.
#[test]
fn glob_on_success_is_a_real_list_of_strings() {
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function f(string $pattern): array {
                $paths = glob($pattern);
                return $paths !== false ? $paths : [];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 32 Finding D: microtime()'s real stub is unconditionally
// `string|float` - real PHPStan narrows a literal `true`/`false`
// argument via its own dynamic return type extension.
#[test]
fn microtime_true_infers_plain_float() {
    let d = diagnostics_for(
        r#"<?php
            function f(): float {
                return microtime(true);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn microtime_false_and_bare_infer_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                return microtime(false);
            }
            function g(): string {
                return microtime();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn microtime_with_a_non_literal_bool_keeps_todays_generic_union() {
    // Must not narrow when the argument isn't a literal true/false -
    // the generic string|float union should still apply, and a bare
    // `float` return should still be flagged as unsound.
    let d = diagnostics_for(
        r#"<?php
            function f(bool $b): float {
                return microtime($b);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the generic union to still apply: {d:?}"
    );
}

// Real corpus shape (CalendarBase::buildNextPrev's own `static fn
// (string $a, string $b): int => version_compare($a, $b)`): the real
// stub declares `version_compare(...): int|bool` unconditionally
// (both PHP-version-gated overloads agree), but the 3rd $operator
// argument's mere *presence* - not its value - is what actually
// determines whether the real return is `bool` or always exactly
// `int`.
#[test]
fn version_compare_without_an_operator_infers_plain_int() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $a, string $b): int {
                return version_compare($a, $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn version_compare_with_an_operator_keeps_todays_generic_union() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $a, string $b): int {
                return version_compare($a, $b, '>=');
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the generic int|bool union to still apply: {d:?}"
    );
}

// Phase 32 Finding J: json_encode(...): string|false - passing
// JSON_THROW_ON_ERROR makes a real failure throw instead of
// returning false.
#[test]
fn json_encode_with_throw_on_error_infers_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return json_encode($x, JSON_THROW_ON_ERROR);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real shape: `UserListPageRenderer`'s own bitwise-OR'd multi-flag
// call - the fix has to walk an arbitrary-depth `|`-chain, not just
// match a bare constant.
#[test]
fn json_encode_with_throw_on_error_in_a_multi_flag_bitor_chain_infers_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return json_encode($x, JSON_HEX_TAG | JSON_HEX_AMP | JSON_THROW_ON_ERROR);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 34 Finding A: curl_getinfo($ch, CONSTANT) has a real,
// per-constant return type real PHPStan already models - the real
// shape driving 16 corpus diagnostics (idcGet, identCurl, and every
// other browser-test curl helper).
#[test]
fn curl_getinfo_response_code_infers_int() {
    let d = diagnostics_for(
        r#"<?php
            function f($ch): int {
                return curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn curl_getinfo_effective_url_infers_string() {
    let d = diagnostics_for(
        r#"<?php
            function f($ch): string {
                return curl_getinfo($ch, CURLINFO_EFFECTIVE_URL);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn curl_getinfo_redirect_url_infers_string_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function f($ch): string|false {
                return curl_getinfo($ch, CURLINFO_REDIRECT_URL);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn curl_getinfo_with_an_unrecognized_constant_keeps_todays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch, CURLINFO_NOT_IN_THE_CURATED_TABLE));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn curl_getinfo_with_no_option_argument_keeps_todays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($ch): void {
                needsString(curl_getinfo($ch));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn json_encode_without_throw_on_error_keeps_todays_generic_union() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return json_encode($x);
            }
            function g($x): string {
                return json_encode($x, JSON_PRETTY_PRINT);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        2,
        "expected the generic string|false union to still apply to both: {d:?}"
    );
}

#[test]
fn stdclass_dynamic_property_reads_are_never_flagged_as_undefined() {
    // Phase 34 Finding D: real shape, FilesystemHelperTest/LangTest -
    // a Pest test double capturing an error message via a dynamic
    // property on a plain `new stdClass()`.
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                $capture = new stdClass();
                $capture->lastMessage = 'hi';
                return $capture->lastMessage;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn stdclass_undefined_method_calls_are_still_flagged() {
    // The control case proving Finding D's exemption is property-
    // access-only, not a blanket carve-out - an undefined *method*
    // call on stdClass is a real fatal error real PHPStan still
    // flags too.
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                $c = new stdClass();
                $c->someMethod();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("someMethod"), "got: {d:?}");
}

#[test]
fn extract_suppresses_undefined_variable_checking_for_the_whole_function() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $data) {
                extract($data);
                return $definitelyNotDefinedAnywhere;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn generated_stub_return_type_catches_a_real_bug() {
    // json_encode() can return false on failure - a well-known real PHP
    // gotcha - so passing it straight to a plain-string param is a
    // genuine bug this real (not hand-approximated) signature catches.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): string { return $s; }
            needsString(json_encode(["a" => 1]));
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsString"));
}

// Phase 32 Finding H, corpus-level check: `end()`'s own real stub
// (`function end(object|array &$array): mixed {}` with `@return
// mixed|false`) used to resolve to the literal `mixed|false` - a
// stray `False` member fails `strtoupper()`'s plain `string` argument
// check on its own, even though the whole union is vacuously
// satisfiable via `Mixed`.
#[test]
fn strtoupper_accepts_ends_own_still_mixed_return_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $items */
            function f(array $items): string {
                return strtoupper(end($items));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 23 Finding 7, real shape: phpstorm-stubs'
// `imagecreatefromstring` has a native return hint updated for PHP
// 8's GD object migration (`: GdImage|false`) but a stale `@return
// resource|GdImage|false` doc comment above it, never revised -
// `resolve_type` used to always prefer the docblock, keeping the
// stale `resource` member even though the native hint (present,
// and not itself mentioning `resource`) is authoritative.
#[test]
fn a_stale_resource_doc_comment_does_not_override_a_narrower_native_hint() {
    let d = diagnostics_for(
        r#"<?php
            class GdImage {}
            /** @return resource|GdImage|false */
            function imagecreatefromstring(string $data): GdImage|false {}
            class ImageGd {
                private GdImage $image;
                public function __construct(string $data) {
                    $img = imagecreatefromstring($data);
                    if ($img === false) {
                        throw new Exception('failed');
                    }
                    $this->image = $img;
                }
                public function get(): GdImage {
                    return $this->image;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 23 Finding 7, real shape: Piwigo\Core\Logger::$fileHandle is
// declared `private mixed $fileHandle = null;` deliberately - PHP has
// no native `resource` type to declare, so the property leans on its
// own `@var resource|null` docblock to carry the real type. An
// earlier, unconditional version of Finding 7's fix incorrectly
// treated `mixed` as an authoritative native hint that contradicts
// the docblock's `resource` member, stripping it down to bare `null`
// (a `!== null` guard, not the real code's `(bool)`-cast guard - a
// cast-wrapped property-access truthy check is a separate, pre-
// existing narrowing gap unrelated to this finding).
#[test]
fn a_mixed_native_hint_does_not_override_a_docblocks_resource_member() {
    let d = diagnostics_for(
        r#"<?php
            class Logger {
                /** @var resource|null */
                private mixed $fileHandle = null;
                public function __construct() {
                    $this->open();
                }
                private function open(): void {
                    $handle = fopen('/tmp/x', 'a');
                    if ($handle !== false) {
                        $this->fileHandle = $handle;
                    }
                }
                public function __destruct() {
                    if ($this->fileHandle !== null) {
                        fclose($this->fileHandle);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 24, real shape: `substr()`/`explode()`/`pg_connect()` have no
// native return-type hint at all, so their only source of a return
// type is a stale `@return` doc comment, gated by
// `#[LanguageLevelTypeAware(['V.V' => 'MODERN'], default: 'LEGACY')]`
// immediately above `function` - previously unparsed entirely, so
// the checker read the pre-8.0 legacy type unconditionally.
#[test]
fn language_level_type_aware_return_type_is_used_over_the_stale_docblock() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param string $string
             * @param int $offset
             * @param ?int $length
             * @return string|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string"], default: "string|false")]
            function substr($string, $offset, $length) {}
            function f(string $s): string {
                return substr($s, 0, -1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn language_level_type_aware_still_returns_an_array_when_the_default_would_have() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param string $separator
             * @param string $string
             * @return string[]|false
             */
            #[LanguageLevelTypeAware(["8.0" => "string[]"], default: "string[]|false")]
            function explode($separator, $string) {}
            function f(string $s): array {
                return explode(',', $s);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real shape: `finfo_file`'s own first parameter has no native hint
// at all, only `#[LanguageLevelTypeAware(['8.1' => 'finfo'], default:
// 'resource')] $finfo` - fixing `finfo_open`'s modernized `finfo`
// return type alone (without this) left every downstream
// `finfo_file($handle, ...)` call newly flagged, since `$handle`'s
// now-correct `finfo` type no longer matched `finfo_file`'s still-
// stale `resource` parameter type.
#[test]
fn language_level_type_aware_on_a_parameter_is_used_over_the_stale_docblock() {
    let d = diagnostics_for(
        r#"<?php
            class finfo {}
            /** @return finfo|false */
            #[LanguageLevelTypeAware(["8.1" => "finfo|false"], default: "resource|false")]
            function finfo_open() {}
            /**
             * @param resource $finfo
             * @return string|false
             */
            function finfo_file(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo, string $filename) {}
            function f(string $path): string {
                $handle = finfo_open();
                if ($handle === false) {
                    return '';
                }
                $result = finfo_file($handle, $path);
                return $result === false ? '' : $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A round of Piwigo-corpus bucketing found Doctrine's own
// AbstractQuery::getResult() native stub declares a bare `mixed`
// return with no docblock refinement at all - real PHPStan's own
// phpstan-doctrine extension gives it a much richer, query-builder-
// aware entity type this project can't replicate, but the narrower
// fact that a non-indexBy()'d query's own result is always a real,
// sequentially-keyed list is real and bounded (verified via
// dumpType(); zero indexBy() call sites anywhere in this corpus).
// Real Piwigo code (TagRepository::findAllTags and many others)
// relies on exactly this via array_map(self::toProjection(...),
// $entities).
#[test]
fn doctrine_query_get_result_is_a_real_list() {
    let d = diagnostics_for(
        r#"<?php
            namespace Doctrine\ORM;
            abstract class AbstractQuery {
                public function getResult(): mixed { return []; }
            }
            class Query extends AbstractQuery {}

            namespace App;
            use Doctrine\ORM\Query;
            class Repo1 {
                public function __construct(private Query $q) {}
                /** @return list<int> */
                public function test(): array {
                    return $this->q->getResult();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Finding 36 from the architecture audit - PHP's implicit Stringable
// conformance for any class declaring __toString() wasn't modeled.

#[test]
fn a_class_declaring_to_string_implicitly_implements_stringable() {
    let d = diagnostics_for(
        r#"<?php
            class Money { public function __toString(): string { return "money"; } }
            function display(string|Stringable $x): void {}
            function test(Money $m): void { display($m); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_without_to_string_still_does_not_satisfy_stringable() {
    let d = diagnostics_for(
        r#"<?php
            class NoToString {}
            function display(string|Stringable $x): void {}
            function test(NoToString $n): void { display($n); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Phase 13 Finding D: real shape from `vendor/php-openapi/openapi`'s
// own `OpenApi` class, which declares `@property Paths $paths` (a
// magic property backed by `__get`/`__set`, no real declared field).
#[test]
fn a_class_level_property_tag_lets_the_magic_property_be_read() {
    let d = diagnostics_for(
        r#"<?php
            class Paths {}
            /**
             * @property Paths $paths
             */
            class OpenApi {}
            function f(OpenApi $o): Paths { return $o->paths; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_class_level_property_tag_is_inherited_by_a_subclass() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property int $magic
             */
            class Base {}
            class Sub extends Base {}
            function f(Sub $s): int { return $s->magic; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_real_declared_property_wins_over_a_same_named_property_tag() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property string $x
             */
            class A { public int $x = 0; }
            function f(A $a): int { return $a->x; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_property_read_tag_flags_an_attempted_write() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property-read int $x
             */
            class A {}
            function f(A $a): void { $a->x = 1; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Phase 77: get_called_class()/get_parent_class() - real, evidenced
// late-static-binding shapes real PHPStan's own
// GetCalledClassDynamicReturnTypeExtension/
// GetParentClassDynamicFunctionReturnTypeExtension already model, this
// project's own stub-only fallback discards. `get_called_class`'s own
// real stub is unconditionally plain `string` (no `false`) - calling it
// outside any class context is a real runtime `Error` real PHPStan
// itself still models as `false` (matching that real extension's own
// fallback), which the stub-only fallback could never catch since plain
// `string` matches everything.

#[test]
fn get_called_class_outside_any_class_is_now_caught_as_a_mismatch() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                return get_called_class();
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the outside-class `false` case to now be flagged: {d:?}"
    );
}

#[test]
fn get_called_class_inside_a_class_still_type_checks_as_a_string() {
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public static function whoAmI(): string {
                    return get_called_class();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_called_class_infers_the_enclosing_classs_own_class_string() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            class Derived extends Base {
                public static function make(): int {
                    return TypedRepository::narrow(get_called_class());
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("Derived")),
        "expected a return-type mismatch naming Derived, got: {d:?}"
    );
}

#[test]
fn get_parent_class_infers_the_real_parent_as_a_class_string() {
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Mid extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            class Bottom extends Mid {
                public static function make(): int {
                    return TypedRepository::narrow(get_parent_class(new self()));
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("return type mismatch") && x.message.contains("Mid")),
        "expected a return-type mismatch naming Mid, got: {d:?}"
    );
}

#[test]
fn get_parent_class_of_a_class_with_no_parent_infers_false() {
    let d = diagnostics_for(
        r#"<?php
            class NoParent {
                public static function whoseParent(): string|false {
                    return get_parent_class(new self());
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_parent_class_of_an_unresolvable_argument_keeps_todays_string_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string|false {
                return get_parent_class($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 77: gettype()/get_debug_type() - real per-kind runtime-string
// answers real PHPStan's own GettypeFunctionReturnTypeExtension/
// GetDebugTypeFunctionReturnTypeExtension already model. Both real stubs
// are unconditionally plain `string`, so no finite literal (or literal
// union) return type could ever have type-checked before this phase -
// `gettype_with_a_known_scalar_union...`/`get_debug_type_of_a_final_
// classs_instance...` below are both genuine "used to always fail, now
// passes" cases.

#[test]
fn gettype_with_a_known_scalar_union_infers_the_exact_literal_union() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return 'integer'|'string'
             */
            function f(int|string $x) {
                return gettype($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn gettype_of_an_unresolvable_value_keeps_todays_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return gettype($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_debug_type_of_a_final_classs_instance_infers_its_own_exact_class_name() {
    let d = diagnostics_for(
        r#"<?php
            final class Foo {}
            /**
             * @return 'Foo'
             */
            function f(Foo $x) {
                return get_debug_type($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_debug_type_of_a_final_classs_instance_still_catches_a_wrong_literal() {
    let d = diagnostics_for(
        r#"<?php
            final class Foo {}
            /**
             * @return 'Bar'
             */
            function f(Foo $x) {
                return get_debug_type($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn get_debug_type_of_a_non_final_classs_instance_is_not_over_narrowed() {
    // A real, evidenced false-positive risk: a non-`final` class's own
    // real instance could be any subclass at runtime, so `get_debug_type`
    // must NOT collapse it down to that exact class's own name (unlike
    // the `final`-class case right above) - if it wrongly did, this
    // wrong-literal declared return type would incorrectly pass instead
    // of being flagged.
    let d = diagnostics_for(
        r#"<?php
            class NotFinal {}
            /**
             * @return 'NotFinal'
             */
            function f(NotFinal $x) {
                return get_debug_type($x);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "a non-final class must not be narrowed to its own exact name: {d:?}"
    );
}

// Phase 77: date()/idate() - a static format-character-to-type table
// (no runtime `date()` simulation), real PHPStan's own
// DateFunctionReturnTypeHelper/IdateFunctionReturnTypeHelper already
// model with a real runtime `date()` call this project has no equivalent
// for.

#[test]
fn date_with_a_purely_numeric_literal_format_infers_a_numeric_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return numeric-string
             */
            function f() {
                return date('Ymd');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn date_with_a_day_name_format_is_not_wrongly_claimed_numeric() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return numeric-string
             */
            function f() {
                return date('l');
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "'l' (a day name like Monday) must not be claimed numeric: {d:?}"
    );
}

#[test]
fn date_with_a_compact_time_format_infers_a_numeric_non_empty_string() {
    // `i`/`s` (minutes/seconds) and `O` (signless-colon GMT offset, e.g.
    // `+0200` - real `is_numeric()` accepts the leading sign) are all
    // still digits-only text, not just the handful of format characters
    // the positive test above already covers.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return numeric-string
             */
            function f() {
                return date('His');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn date_with_a_day_name_format_still_infers_a_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return non-empty-string
             */
            function f() {
                return date('l');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn idate_with_a_known_format_character_infers_its_real_bounded_range() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return int<0, 23>
             */
            function f() {
                return idate('H');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn idate_with_an_uncovered_format_character_keeps_todays_int_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int|false {
                return idate('Y');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 77: pathinfo()/parse_url()/sscanf() - hardcoded constant-
// array-shape tables real PHPStan's own PathinfoFunctionDynamicReturnTypeExtension/
// ParseUrlFunctionDynamicReturnTypeExtension/SscanfFunctionDynamicReturnTypeExtension
// already model.

#[test]
fn pathinfo_with_one_argument_infers_the_real_array_shape() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return array{dirname: string, basename: string, extension?: string, filename: string}
             */
            function f(string $path) {
                return pathinfo($path);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pathinfo_with_a_single_component_flag_narrows_to_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $path): string {
                return pathinfo($path, PATHINFO_EXTENSION);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn pathinfo_with_a_non_literal_flag_keeps_todays_array_or_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $path, int $flags): array|string {
                return pathinfo($path, $flags);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn parse_url_with_a_known_component_constant_infers_its_real_type() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return int<0, 65535>|false|null
             */
            function f(string $url) {
                return parse_url($url, PHP_URL_PORT);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn parse_url_with_one_argument_infers_the_components_array_or_false() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false
             */
            function f(string $url) {
                return parse_url($url);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sscanf_with_a_literal_format_infers_a_typed_tuple_shape() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return array{0: int|null, 1: string|null}|null
             */
            function f(string $line) {
                return sscanf($line, '%d %s');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sscanf_with_a_non_literal_format_keeps_todays_array_or_int_or_null() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $line, string $format): array|int|null {
                return sscanf($line, $format);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 77: stat()/lstat()/fstat()/dio_stat() - a hardcoded constant-
// array-shape table real PHPStan's own StatDynamicReturnTypeExtension/
// DioStatDynamicFunctionReturnTypeExtension already model.

#[test]
fn stat_infers_the_real_key_shape_or_false() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return array{dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false
             */
            function f(string $path) {
                return stat($path);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn dio_stat_infers_the_real_key_shape_or_null() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return array{device: int, inode: int, mode: int, nlink: int, uid: int, gid: int, device_type: int, size: int, blocksize: int, blocks: int, atime: int, mtime: int, ctime: int}|null
             */
            function f($fd) {
                return dio_stat($fd);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 77: hash_algos()/openssl_get_cipher_methods()/mb_list_encodings()
// - each really is always a non-empty list of non-empty strings,
// regardless of host/PHP version/installed extensions (deliberately NOT
// the exact literal snapshot real PHPStan's own HashFunctionsReturnTypeExtension/
// OpenSslCipherMethodsProvider/MbFunctionsReturnTypeExtensionTrait build
// by calling these for real at its own analysis time to validate *other*
// functions' string arguments - see the match arm's own doc comment for
// why that heavier mechanism isn't replicated here).

#[test]
fn hash_algos_infers_a_non_empty_list_of_non_empty_strings() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return non-empty-list<non-empty-string>
             */
            function f() {
                return hash_algos();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn openssl_and_mbstring_capability_lists_share_the_same_shape() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return non-empty-list<non-empty-string>
             */
            function ciphers() {
                return openssl_get_cipher_methods();
            }
            /**
             * @return non-empty-list<non-empty-string>
             */
            function encodings() {
                return mb_list_encodings();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 77: curl_getinfo()'s own per-option table, extended from Phase
// 34 Finding A's original ~20 curated entries to the real, full ~73-entry
// table (transcribed from real PHPStan's own
// CurlGetinfoFunctionDynamicReturnTypeExtension - see
// `curl_getinfo_component_type`'s own doc comment).

#[test]
fn curl_getinfo_certinfo_infers_a_nested_string_map_list() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @return list<array<string, string>>
             */
            function f($ch) {
                return curl_getinfo($ch, CURLINFO_CERTINFO);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn curl_getinfo_used_proxy_infers_int_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function f($ch): int|false {
                return curl_getinfo($ch, CURLINFO_USED_PROXY);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
