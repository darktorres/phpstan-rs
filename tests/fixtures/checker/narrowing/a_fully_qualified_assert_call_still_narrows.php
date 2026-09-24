<?php
            function needsInt(int $x): int { return $x; }
            function f(int|string $input) {
                \assert(\is_int($input));
                needsInt($input);
            }
        