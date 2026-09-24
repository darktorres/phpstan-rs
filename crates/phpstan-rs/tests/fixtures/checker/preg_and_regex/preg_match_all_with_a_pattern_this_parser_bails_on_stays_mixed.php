<?php
            function f($subject): void {
                preg_match_all('/(x)(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        