<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(cat)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsInt($matches[1][0][0]);
                needsInt($matches[1][0][1]);
            }
        