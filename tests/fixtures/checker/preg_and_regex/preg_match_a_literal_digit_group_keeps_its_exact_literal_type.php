<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(1|2)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        