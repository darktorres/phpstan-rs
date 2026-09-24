<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/([a-z]+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        