<?php
            /** @param int<0, 999999998> $ns */
            function needsNarrower(int $ns): void {}
            function test(): void {
                [$s, $ns] = hrtime();
                needsNarrower($ns);
            }
        