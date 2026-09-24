<?php
            function f(string $subject): string {
                return substr_replace($subject, 'x', 0, 1);
            }
        