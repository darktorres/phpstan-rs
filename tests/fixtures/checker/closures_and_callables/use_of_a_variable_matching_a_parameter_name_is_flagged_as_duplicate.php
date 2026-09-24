<?php
            function test(): void {
                $x = 1;
                $fn = function ($x) use ($x) {
                    return $x;
                };
                $fn(2);
            }
        