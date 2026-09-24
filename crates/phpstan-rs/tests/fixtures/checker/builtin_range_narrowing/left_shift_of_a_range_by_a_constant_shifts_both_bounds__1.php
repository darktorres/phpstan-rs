<?php
            /** @param int<4, 39> $x */
            function needsTighter(int $x): void {}
            /** @param int<1, 10> $x */
            function test(int $x): void {
                needsTighter($x << 2);
            }
        