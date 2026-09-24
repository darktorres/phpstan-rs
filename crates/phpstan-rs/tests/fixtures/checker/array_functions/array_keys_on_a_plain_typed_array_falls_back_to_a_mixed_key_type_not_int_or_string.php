<?php
            /** @param array<string, string> $classMap */
            function f(array $classMap): void {
                foreach (array_keys($classMap) as $symbol) {
                    str_starts_with($symbol, 'x');
                }
            }
        