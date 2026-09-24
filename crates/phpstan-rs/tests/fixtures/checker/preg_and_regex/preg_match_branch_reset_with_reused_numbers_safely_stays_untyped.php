<?php
            function f($subject): void {
                preg_match('/(?|(\d+)|([a-z]+))/', $subject, $matches);
                echo $matches[1];
            }
        