<?php
            function needsPair(string $a, string $b): void {}
            /**
             * @param array{0: string, 1: string} $pair
             * @param array{range: array{0: int, 1: int}} $range
             */
            function handle(array $pair, array $range, bool $cond): void {
                $x = $cond ? $pair : $range;
                if (array_key_exists(0, $x)) {
                    needsPair($x[0], $x[1]);
                }
            }
        