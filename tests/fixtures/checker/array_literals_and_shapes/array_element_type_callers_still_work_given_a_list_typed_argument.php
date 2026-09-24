<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach (array_map(fn($v) => $v + 1, $x) as $v) {
                    needsInt($v);
                }
                foreach (array_chunk($x, 2) as $chunk) {
                    foreach ($chunk as $v) {
                        needsInt($v);
                    }
                }
                needsInt(array_reduce($x, fn($carry, $v) => $carry + $v, 0));
            }
            function needsInt(int $x): int { return $x; }
        