<?php
            function accepts(int $i): void {}
            /** @param int<0, 5> $shift */
            function test(int $x, int $shift): void {
                accepts($x << $shift);
            }
        