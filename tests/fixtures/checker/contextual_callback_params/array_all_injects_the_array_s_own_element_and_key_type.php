<?php
            function needsInt(int $i): void {}
            /** @param array<string, int> $items */
            function f(array $items): bool {
                return array_all($items, function ($value, $key) {
                    needsInt($key);
                    return true;
                });
            }
        