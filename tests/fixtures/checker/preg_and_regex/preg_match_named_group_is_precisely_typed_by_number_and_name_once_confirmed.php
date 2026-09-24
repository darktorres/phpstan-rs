<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(?<year>\d+)/', $subject, $matches)) {
                    needsArray($matches[1]);
                    needsArray($matches['year']);
                }
            }
        