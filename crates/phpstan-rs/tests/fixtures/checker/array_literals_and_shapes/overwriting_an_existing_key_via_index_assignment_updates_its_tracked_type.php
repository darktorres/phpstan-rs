<?php
            function needsInt(int $x): int { return $x; }
            function test(): int {
                $x = ['a' => 1];
                $x['a'] = 5;
                return needsInt($x['a']);
            }
        