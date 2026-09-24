<?php
            function f(int $n): string {
                try {
                    return match ($n) {
                        1 => "one",
                    };
                } catch (\Throwable $e) {
                    return "caught";
                }
            }
        