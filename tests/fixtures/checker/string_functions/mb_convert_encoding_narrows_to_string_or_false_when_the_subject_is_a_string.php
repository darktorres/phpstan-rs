<?php
            function f(string $subject): string|false {
                return mb_convert_encoding($subject, 'UTF-8', 'ISO-8859-1');
            }
        