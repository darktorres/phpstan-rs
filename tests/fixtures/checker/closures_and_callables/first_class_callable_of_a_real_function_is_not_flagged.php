<?php
            function real(int $a, int $b): int { return $a + $b; }
            function test(): void {
                $f = real(...);
            }
        