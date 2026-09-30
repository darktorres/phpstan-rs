<?php
            function needsString(string $s): void {}
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE) === 1) {
                    needsString($matches[1][0]);
                    needsInt($matches[1][1]);
                }
            }
        