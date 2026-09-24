<?php
            /** @param list<int> $x */
            function mutates(array &$x): void {}
            function f(): void {
                $items = [];
                mutates($items);
                foreach ($items as $item) {
                    needsInt($item);
                }
            }
            function needsInt(int $x): int { return $x; }
        