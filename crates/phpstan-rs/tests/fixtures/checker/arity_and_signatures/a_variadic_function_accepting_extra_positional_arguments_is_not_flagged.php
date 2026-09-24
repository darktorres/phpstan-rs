<?php
            function withVariadic40(int $a, int ...$rest): int { return $a; }
            function test(): int { return withVariadic40(1, 2, 3, 4, 5); }
        