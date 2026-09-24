<?php
            /**
             * @template T
             * @param T[] $items
             * @return T
             */
            function firstOf(array $items) { return $items[0]; }
            function needsInt(int $x): int { return $x; }
            /** @param string[] $words */
            function process(array $words) {
                return needsInt(firstOf($words));
            }
        