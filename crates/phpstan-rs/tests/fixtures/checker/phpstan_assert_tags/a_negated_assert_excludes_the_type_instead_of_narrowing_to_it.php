<?php
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
        