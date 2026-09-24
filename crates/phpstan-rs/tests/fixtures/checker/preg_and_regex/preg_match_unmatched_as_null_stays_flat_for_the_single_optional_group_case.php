<?php
            function f($subject): void {
                preg_match('/(a(b))?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                echo $matches[1];
                echo $matches[2];
            }
        