<?php
            /** @param int<0, 100> $x */
            function needsBounded(int $x): void {}
            function test(int $min): void {
                needsBounded(random_int($min, 10));
            }
        