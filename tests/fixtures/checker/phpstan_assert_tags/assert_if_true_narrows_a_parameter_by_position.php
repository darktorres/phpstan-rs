<?php
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
        