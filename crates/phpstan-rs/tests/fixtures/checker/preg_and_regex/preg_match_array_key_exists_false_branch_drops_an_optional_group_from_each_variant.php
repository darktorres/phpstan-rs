<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                if (! array_key_exists('2', $matches)) {
                    needsInt($matches);
                }
            }
        