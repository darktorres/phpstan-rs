<?php
            function f(int $x): string {
                return match (true) {
                    $x > 5 => "big",
                    $x > 10 => "bigger",
                };
            }
        