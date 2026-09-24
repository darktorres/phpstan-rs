<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    needsString($oneMatch[1]);
                }
            }
        