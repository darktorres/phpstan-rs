<?php
            use function PHPStan\Testing\assertType;
            final class C {
                private const array ACTIONS = ['a', 'b'];
                /** @param list<int> $ids */
                public function f(string $s, int $n, array $ids): void {
                    if (! in_array($s, self::ACTIONS, true)) {
                        return;
                    }
                    assertType("'a'|'b'", $s);
                    if (in_array($n, $ids, true)) {
                        assertType('int', $n);
                    }
                }
            }
        