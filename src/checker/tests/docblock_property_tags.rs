//! PHPDoc annotation semantics: docblock-vs-native-hint precedence, and `@property`
//! tag support (magic-property read/write/inheritance/unknown-class checks).

use super::helpers::*;

// A stale `@return resource|GdImage|false` doc comment sitting above a
// native return hint that's already been updated (`: GdImage|false`)
// must not override the native hint - `resolve_type` must not always
// prefer the docblock, keeping the stale `resource` member even though
// the native hint (present, and not itself mentioning `resource`) is
// authoritative.
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
    // The stale tag itself is still reported, as real PHPStan's
    // `IncompatiblePhpDocTypeRule` does; nothing downstream is.
    let ids: Vec<&str> = d.iter().map(|x| x.identifier.as_str()).collect();
    assert_eq!(ids, ["return.phpDocType"], "got: {d:?}");
}

// A property deliberately declared `private mixed $fileHandle = null;`
// (PHP has no native `resource` type to declare) leans on its `@var
// resource|null` docblock to carry the real type - `mixed` must not be
// treated as an authoritative native hint that contradicts the
// docblock's `resource` member, stripping it down to bare `null`.
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

// `substr()`/`explode()`/`pg_connect()` have no native return-type hint
// at all, so their only source of a return type is a stale `@return`
// doc comment, gated by `#[LanguageLevelTypeAware(['V.V' => 'MODERN'],
// default: 'LEGACY')]` immediately above `function` - this attribute
// must be parsed, or the checker reads the pre-8.0 legacy type
// unconditionally.
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

// Doctrine's `AbstractQuery::getResult()` native stub declares a bare
// `mixed` return with no docblock refinement at all - PHPStan's
// phpstan-doctrine extension gives it a much richer, query-builder-aware
// entity type this project can't replicate, but the narrower fact that a
// non-indexBy()'d query's result is always a sequentially-keyed list is
// bounded enough to model directly.
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

// PHP's implicit Stringable conformance for any class declaring
// __toString() must be modeled.

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

// A class-level `@property Paths $paths` tag (a magic property backed
// by `__get`/`__set`, no real declared field).
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

// @property/@property-read/@property-write's validation half (mirrors
// PHPStan's PropertyTagCheck) - the tag's declared type must reference a
// real, known class, the same existence-checking every native property
// type hint already gets.

#[test]
fn a_property_tag_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property TotallyMadeUp $thing
             */
            class A {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"
            && x.message.contains("TotallyMadeUp")
            && x.message.contains("@property")),
        "got: {d:?}"
    );
}

#[test]
fn a_property_read_tag_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property-read TotallyMadeUp $thing
             */
            class A {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("@property-read")),
        "got: {d:?}"
    );
}

#[test]
fn a_property_write_tag_referencing_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property-write TotallyMadeUp $thing
             */
            class A {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "class.notFound" && x.message.contains("@property-write")),
        "got: {d:?}"
    );
}

#[test]
fn a_property_tag_referencing_a_real_class_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Paths {}
            /**
             * @property Paths $paths
             */
            class OpenApi {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_property_tag_with_a_builtin_scalar_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property int $x
             * @property-read string $y
             */
            class A {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unknown_class_referenced_only_by_a_property_tag_shadowed_by_a_real_property_is_not_double_flagged()
 {
    // Documented, deliberate leniency: a tag's name shadowed by a real,
    // physically-declared property of the same name checks the real
    // property's own type, not the tag's.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @property TotallyMadeUp $x
             */
            class A { public int $x = 0; }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}
