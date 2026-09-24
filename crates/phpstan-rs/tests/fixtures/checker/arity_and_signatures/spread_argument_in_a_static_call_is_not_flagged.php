<?php
            class Calc { public static function multiply(int $a, int $b): int { return $a * $b; } }
            function test(array $args): int { return Calc::multiply(...$args); }
        