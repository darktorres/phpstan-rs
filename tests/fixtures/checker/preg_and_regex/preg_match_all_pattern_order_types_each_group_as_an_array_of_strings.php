<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                needsInt($matches[1]);
            }
        