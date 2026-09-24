<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    needsArray($matches[1]);
                }
            }
        