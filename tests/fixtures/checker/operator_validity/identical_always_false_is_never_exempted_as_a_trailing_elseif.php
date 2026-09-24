<?php
            function f(int $x): void {
                $x = 5;
                if ($x > 100) {
                    echo "a";
                } elseif ($x === "never") {
                    echo "b";
                }
            }
        