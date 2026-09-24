<?php
            function f(int $n): string {
                try {
                    return match ($n) {
                        1 => "one",
                    };
                } catch (\UnhandledMatchError $e) {
                    return "caught";
                }
            }
        