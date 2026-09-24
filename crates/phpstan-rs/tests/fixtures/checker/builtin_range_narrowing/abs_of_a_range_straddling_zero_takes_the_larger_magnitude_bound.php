<?php
            /** @param int<0, 9> $x */
            function needsNarrower(int $x): void {}
            /** @param int<-3, 10> $x */
            function test(int $x): void {
                needsNarrower(abs($x));
            }
        