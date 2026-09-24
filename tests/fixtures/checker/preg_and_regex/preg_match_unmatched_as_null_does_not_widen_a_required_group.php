<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                needsString($matches[1]);
            }
        