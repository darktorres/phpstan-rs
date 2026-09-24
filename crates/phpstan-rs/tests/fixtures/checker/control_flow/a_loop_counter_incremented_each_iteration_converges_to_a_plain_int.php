<?php
            function needsArray(array $x): void {}
            function f() {
                $i = 0;
                while (rand(0, 1)) {
                    $i = $i + 1;
                }
                needsArray($i);
            }
        