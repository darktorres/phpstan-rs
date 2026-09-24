<?php
            /** @param array<string, int> $a
             *  @return list<int>
             */
            function f(array $a): array {
                usort($a, static fn (int $x, int $y): int => $x <=> $y);
                return $a;
            }
            /** @param array<string, int> $b
             *  @return list<int>
             */
            function g(array $b): array {
                shuffle($b);
                return $b;
            }
        