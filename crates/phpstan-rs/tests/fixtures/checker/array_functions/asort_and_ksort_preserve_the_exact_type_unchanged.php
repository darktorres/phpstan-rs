<?php
            /** @param array{a: int, b: string} $x
             *  @return array{a: int, b: string}
             */
            function f(array $x): array {
                asort($x);
                return $x;
            }
            /** @param array{a: int, b: string} $y
             *  @return array{a: int, b: string}
             */
            function g(array $y): array {
                ksort($y);
                return $y;
            }
        