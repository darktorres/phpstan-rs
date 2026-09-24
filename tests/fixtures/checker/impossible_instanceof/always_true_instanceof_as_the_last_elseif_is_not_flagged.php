<?php
            interface Shape {}
            class Square implements Shape {}
            class Circle implements Shape {}
            function f(Square|Circle $s): void {
                if ($s instanceof Square) {
                    echo 1;
                } elseif ($s instanceof Circle) {
                    echo 2;
                }
            }
        