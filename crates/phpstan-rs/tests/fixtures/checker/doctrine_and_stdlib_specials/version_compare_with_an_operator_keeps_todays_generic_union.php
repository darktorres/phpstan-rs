<?php
            function f(string $a, string $b): int {
                return version_compare($a, $b, '>=');
            }
        