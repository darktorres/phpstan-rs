<?php
            function needsString(string $s): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL) === 1) {
                    needsString($matches[1]);
                }
            }
        