<?php
            function needsExact(int $x): void {}
            /** @param int<-10, -1> $x */
            function test(int $x): void {
                $y = abs($x);
                /** @var int<1, 10> $y */
                needsExact($y);
            }
        