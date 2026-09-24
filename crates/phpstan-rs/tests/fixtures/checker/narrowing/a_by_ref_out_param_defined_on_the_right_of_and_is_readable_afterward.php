<?php
            function f(mixed $subject): void {
                if (is_string($subject) and (bool) preg_match('/(\d+)/', $subject, $matches)) {
                    echo $matches[0];
                    echo $matches[1];
                }
            }
        