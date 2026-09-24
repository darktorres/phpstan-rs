<?php
            function f($subject): void {
                if (preg_match('/(?:(cat)|(dog))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches) && !array_key_exists('2', $matches)) {
                        echo $matches[1];
                    }
                }
            }
        