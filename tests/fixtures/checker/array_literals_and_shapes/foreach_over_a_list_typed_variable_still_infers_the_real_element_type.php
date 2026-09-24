<?php
            /** @param list<int> $x */
            function f($x): void {
                foreach ($x as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        