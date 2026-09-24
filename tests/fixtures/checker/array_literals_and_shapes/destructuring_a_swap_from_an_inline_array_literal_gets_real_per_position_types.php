<?php
            function needsString(string $s): void {}
            function test(): void {
                $a = 1;
                $b = "x";
                [$a, $b] = [$b, $a];
                needsString($b);
            }
        