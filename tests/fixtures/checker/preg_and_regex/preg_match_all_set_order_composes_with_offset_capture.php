<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)/', $subject, $matches, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
                foreach ($matches as $oneMatch) {
                    needsInt($oneMatch[1][1]);
                }
            }
        