<?php
            /** @param int<-11, -2> $x */
            function needsNarrower(int $x): void {}
            /** @param int<0, 10> $x */
            function test(int $x): void {
                needsNarrower(~$x);
            }
        