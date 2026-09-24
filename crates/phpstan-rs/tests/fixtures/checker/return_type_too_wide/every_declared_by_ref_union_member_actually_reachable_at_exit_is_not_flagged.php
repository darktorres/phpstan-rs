<?php
            function f(int|string &$x, bool $cond): void {
                if ($cond) {
                    $x = 1;
                } else {
                    $x = 'a string';
                }
            }
        