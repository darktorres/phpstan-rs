<?php
            class Assert {
                public static function string(mixed $value): void {
                    if (!is_string($value)) {
                        throw new \InvalidArgumentException('not a string');
                    }
                }
            }

            function takesString(string $s): void {}

            function test(int|string $input): void {
                Assert::string($input);
                takesString($input);
            }
        