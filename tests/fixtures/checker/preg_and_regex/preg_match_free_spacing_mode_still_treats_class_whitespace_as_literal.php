<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/([a b])/x', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        