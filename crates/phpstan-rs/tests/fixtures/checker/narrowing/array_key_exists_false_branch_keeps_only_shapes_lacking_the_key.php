<?php
            function needsAValue(int $a): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (! array_key_exists('b', $x)) {
                    needsAValue($x['a']);
                }
            }
        