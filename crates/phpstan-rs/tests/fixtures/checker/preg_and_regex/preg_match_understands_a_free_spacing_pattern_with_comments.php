<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/
                    (\d+)   # the leading number
                    \s*
                    ([a-z]+) # the trailing word
                /x', $subject, $matches)) {
                    needsInt($matches[1]);
                    needsInt($matches[2]);
                }
            }
        