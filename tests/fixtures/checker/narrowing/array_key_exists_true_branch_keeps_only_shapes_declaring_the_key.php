<?php
            function needsBValue(string $b): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (array_key_exists('b', $x)) {
                    needsBValue($x['b']);
                }
            }
        