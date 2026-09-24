<?php
            function needsInt(int $x): int { return $x; }
            /** @param string $s */
            function f($s) {
                return needsInt($s);
            }
        