<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(cat)|(dog)|(bird)/', $subject, $matches)) {
                    if (array_key_exists('2', $matches) && !array_key_exists('3', $matches)) {
                        needsInt($matches[1]); // earlier sibling - forced ''
                        needsInt($matches[2]); // this branch's own group
                        echo $matches[3]; // later sibling - removed entirely
                    }
                }
            }
        