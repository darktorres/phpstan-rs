<?php
            function needsString(string $s): void {}
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsString($matches[1][0]);
                needsInt($matches[1][1]);
            }
        