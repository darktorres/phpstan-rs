<?php
            function g(bool $flag): void {}
            function f(bool|int $x): void {
                if ($x == 0) {
                    g($x);
                }
            }
        