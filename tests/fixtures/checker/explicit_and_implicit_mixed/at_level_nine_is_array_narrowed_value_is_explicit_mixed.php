<?php
            function f($m): void {
                if (is_array($m)) {
                    g($m[0]);
                }
            }
            function g(string $s): void {}
        