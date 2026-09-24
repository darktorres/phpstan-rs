<?php
            function f(int $a, string $b): void {}
            /** @param array<string> $values */
            function test(array $values): void {
                f(...$values);
            }
        