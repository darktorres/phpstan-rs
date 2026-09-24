<?php
            function needsSeconds(int $s): void {}
            function needsNanoseconds(int $ns): void {}
            function test(): void {
                [$s1, $ns1] = hrtime();
                [$s2, $ns2] = hrtime(false);
                needsSeconds($s1);
                needsSeconds($s2);
                needsNanoseconds($ns1);
                needsNanoseconds($ns2);
            }
        