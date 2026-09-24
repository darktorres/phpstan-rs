<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $xs */
            function f(string $a, string $b, array $xs): void {
                /** @psalm-var non-empty-string $a */
                $a = str_replace('_', '-', $a);
                assertType('non-empty-string', $a);
                /** @phan-var non-empty-string $b */
                $b = str_replace('_', '-', $b);
                assertType('non-empty-string', $b);
                /** @var non-empty-string $k */
                foreach ($xs as $k => $v) {
                    assertType('non-empty-string', $k);
                }
            }
        