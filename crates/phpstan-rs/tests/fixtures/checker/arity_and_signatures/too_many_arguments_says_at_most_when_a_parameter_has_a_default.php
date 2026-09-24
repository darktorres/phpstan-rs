<?php
            function withDefault40(int $a, int $b = 5): int { return $a + $b; }
            function test(): int { return withDefault40(1, 2, 3); }
        