<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(?|(\d+)|x|y)([a-z]+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                    needsInt($matches[2]);
                }
            }
        