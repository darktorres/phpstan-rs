<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                echo $matches[99];
            }
        