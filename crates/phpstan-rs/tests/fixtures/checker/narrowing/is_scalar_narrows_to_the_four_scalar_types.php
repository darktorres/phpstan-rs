<?php
            function needsInt(int $x): int { return $x; }
            function process(mixed $value) {
                if (is_scalar($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        