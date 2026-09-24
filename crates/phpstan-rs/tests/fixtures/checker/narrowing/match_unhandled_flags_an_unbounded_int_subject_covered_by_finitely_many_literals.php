<?php
            function f(int $n): string {
                return match ($n) {
                    1 => "one",
                    2 => "two",
                };
            }
        