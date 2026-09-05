//! `@phpstan-assert`/`-if-true`/`-if-false` tags - real PHPStan's
//! custom-assertion-function convention. Real corpus shape: phpunit's
//! own accessor pair `hasTestFilesFile(): bool` (tagged `@phpstan-assert
//! -if-true !null $this->testFilesFile`) then `testFilesFile(): string`'s
//! own body guarding on it.

use super::helpers::*;

#[test]
fn assert_if_true_on_a_method_narrows_a_this_property_in_the_true_branch() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $testFilesFile = null;

                /** @phpstan-assert-if-true !null $this->testFilesFile */
                public function hasTestFilesFile(): bool {
                    return $this->testFilesFile !== null;
                }

                public function testFilesFile(): string {
                    if (!$this->hasTestFilesFile()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->testFilesFile;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_the_assert_tag_the_same_shape_is_still_flagged() {
    // Guards against the test above passing for an unrelated reason -
    // confirms the tag itself is what's doing the narrowing.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $testFilesFile = null;

                public function hasTestFilesFile(): bool {
                    return $this->testFilesFile !== null;
                }

                public function testFilesFile(): string {
                    if (!$this->hasTestFilesFile()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->testFilesFile;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return-type mismatch (nullable vs string) with no tag, got: {d:?}"
    );
}

#[test]
fn assert_if_false_on_a_method_narrows_in_the_false_branch() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $x = null;

                /** @phpstan-assert-if-false !null $this->x */
                public function isXNull(): bool {
                    return $this->x === null;
                }

                public function needsX(): string {
                    if ($this->isXNull()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_if_true_narrows_a_parameter_by_position() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert-if-true !null $value */
                public static function notNull($value): bool {
                    return $value !== null;
                }
            }
            function f(?string $x): string {
                if (!Assert::notNull($x)) {
                    throw new \RuntimeException('none');
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unconditional_assert_tag_narrows_unconditionally_after_the_call() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert string $value */
                public static function string($value): void {
                    if (!is_string($value)) {
                        throw new \RuntimeException('not a string');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(mixed $x): void {
                Assert::string($x);
                needsString($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_negated_assert_excludes_the_type_instead_of_narrowing_to_it() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert !null $value */
                public static function notNull($value): void {
                    if ($value === null) {
                        throw new \RuntimeException('was null');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(?string $x): void {
                Assert::notNull($x);
                needsString($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
