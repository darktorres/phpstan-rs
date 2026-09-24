<?php
            function f($pattern, $subject): void {
                preg_match($pattern, $subject, $matches);
                echo $matches[99];
            }
        