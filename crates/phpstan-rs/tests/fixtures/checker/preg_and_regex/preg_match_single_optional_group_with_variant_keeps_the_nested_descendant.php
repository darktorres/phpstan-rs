<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (array_key_exists('1', $matches)) {
                        needsInt($matches[1]);
                        needsInt($matches[2]);
                    }
                }
            }
        