<?php
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
        