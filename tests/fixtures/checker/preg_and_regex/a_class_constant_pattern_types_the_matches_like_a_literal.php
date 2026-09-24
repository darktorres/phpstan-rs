<?php
            use function PHPStan\Testing\assertType;
            final class Op {
                private const PATTERN = '/^(?<op>[.])?(?<rest>.*)$/';
                public static function f(string $e): void {
                    if (preg_match(self::PATTERN, $e, $m)) {
                        assertType("array{0: string, 1: non-empty-string|'', op: non-empty-string|'', 2: string, rest: string}", $m);
                    }
                }
            }
        