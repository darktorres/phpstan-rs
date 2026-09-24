<?php
            function f(string $prefix, $subject): void {
                preg_match($prefix . '(\d+)/', $subject, $matches);
                echo $matches[99];
            }
        