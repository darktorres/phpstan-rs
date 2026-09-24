<?php
            /**
             * @throws \UnhandledMatchError
             */
            function f(int $n): string {
                return match ($n) {
                    1 => "one",
                };
            }
        