<?php
            function f4(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return f4(a: 1); }
        