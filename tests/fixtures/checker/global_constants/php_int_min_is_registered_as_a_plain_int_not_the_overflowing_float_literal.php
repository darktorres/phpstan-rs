<?php
            define('PHP_INT_MIN', -9223372036854775808);
            define('PHP_INT_MAX', 9223372036854775807);
            function run(): int {
                return random_int(PHP_INT_MIN, PHP_INT_MAX);
            }
        