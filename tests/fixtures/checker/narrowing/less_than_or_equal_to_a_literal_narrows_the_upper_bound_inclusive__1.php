<?php
            /** @param int<0, 9> $x */
            function needsTighter(int $x): void {}
            function test(int $x): void {
                if ($x <= 10 && $x >= 0) {
                    needsTighter($x);
                }
            }
        