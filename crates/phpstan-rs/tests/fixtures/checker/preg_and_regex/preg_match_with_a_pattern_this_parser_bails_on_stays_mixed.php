<?php
            function f($subject): void {
                preg_match('/(x)(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        