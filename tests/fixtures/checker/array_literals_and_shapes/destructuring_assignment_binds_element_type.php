<?php
            function needsInt(int $x): int { return $x; }
            /** @param string[] $pair */
            function f(array $pair) {
                [$a, $b] = $pair;
                return needsInt($a);
            }
        