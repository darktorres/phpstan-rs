<?php
            function sink2(int $x): void {}
            function f(?int $x): void {
                $y = $x;
                if ($y) {
                    sink2($x);
                }
            }
        