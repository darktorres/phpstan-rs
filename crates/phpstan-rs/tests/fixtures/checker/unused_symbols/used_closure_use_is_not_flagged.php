<?php
            function makeAdder(int $base) {
                return function (int $x) use ($base) {
                    return $x + $base;
                };
            }
        