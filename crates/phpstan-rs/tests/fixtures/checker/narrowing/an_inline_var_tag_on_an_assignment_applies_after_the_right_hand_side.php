<?php
            use function PHPStan\Testing\assertType;
            /** @param array<mixed> $sel */
            function f(array $sel): void {
                /** @var list<int|string> $sel */
                $sel = array_values(array_filter(
                    $sel,
                    static fn ($item): bool => is_int($item) || is_string($item),
                ));
                assertType('list<int|string>', $sel);
            }
            function g(): void {
                /** @var int $a */
                [$a, $b] = [rand() ? 1 : 'x', 2];
                assertType('int', $a);
            }
        