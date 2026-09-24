<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, SOME_UNDEFINED_FLAG);
                echo $matches[99];
            }
        