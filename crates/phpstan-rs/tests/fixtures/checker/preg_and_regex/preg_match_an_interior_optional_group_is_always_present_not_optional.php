<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?(c)/', $subject, $matches)) {
                    if (! array_key_exists('2', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        