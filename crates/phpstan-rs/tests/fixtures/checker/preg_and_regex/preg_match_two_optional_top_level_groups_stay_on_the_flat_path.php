<?php
            function f($subject): void {
                preg_match('/(a)?(b)?/', $subject, $matches);
                echo $matches[1];
                echo $matches[2];
            }
        