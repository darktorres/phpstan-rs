<?php
            function f(int &$x): void {
                $cb = fn() => $x = 'not an int';
                $cb();
            }
        