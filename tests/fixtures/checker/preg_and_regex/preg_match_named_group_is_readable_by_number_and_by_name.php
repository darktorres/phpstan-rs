<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(?<year>\d+)/', $subject, $matches);
                needsString($matches[1]);
                needsString($matches['year']);
            }
        