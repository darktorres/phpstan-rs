<?php
            function f3(int $a, int $b, int $c): int { return $a + $b + $c; }
            function test(): int { return f3(1, c: 3, b: 2); }
        