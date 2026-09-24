<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    echo $oneMatch[5];
                }
            }
        