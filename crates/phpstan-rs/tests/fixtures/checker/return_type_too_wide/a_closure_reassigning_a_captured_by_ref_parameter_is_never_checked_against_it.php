<?php
            function f(int &$x): void {
                $cb = function () use (&$x) {
                    $x = 'not an int';
                };
                $cb();
            }
        