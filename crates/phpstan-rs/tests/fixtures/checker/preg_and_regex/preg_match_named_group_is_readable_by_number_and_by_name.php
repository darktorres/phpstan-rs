<?php
            function needsString(string $s): void {}
            function f($subject): void {
                if (preg_match('/(?<year>\d+)/', $subject, $matches) === 1) {
                    needsString($matches[1]);
                    needsString($matches['year']);
                }
            }
        