<?php
            function needsTwo(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = needsTwo(...);
            }
        