<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): int { return needsTwo(1, 2, 3, 4, 5); }
        