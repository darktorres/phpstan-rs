<?php
            /** @param int<1, 9> $x */
            function needsNarrower(int $x): void {}
            /** @param int<-10, -1> $x */
            function test(int $x): void {
                needsNarrower(abs($x));
            }
        