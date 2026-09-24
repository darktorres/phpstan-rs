<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                needsString($matches[1]);
            }
        