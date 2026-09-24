<?php
            function f(int &$x): void {
                ['a' => $x] = ['a' => 1];
            }
        