<?php
            function needsThree(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(array $args): int { return needsThree(...$args); }
        