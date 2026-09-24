<?php
            function needsInt(int $i): void {}
            /** @param array<string, int> $items */
            function f(array $items): void {
                array_walk($items, function ($value, $key) {
                    needsInt($key);
                });
            }
        