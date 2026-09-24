<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(\d+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        