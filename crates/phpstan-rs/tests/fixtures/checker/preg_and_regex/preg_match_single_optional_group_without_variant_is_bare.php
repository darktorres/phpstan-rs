<?php
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches)) {
                        echo $matches[1];
                        echo $matches[2];
                    }
                }
            }
        