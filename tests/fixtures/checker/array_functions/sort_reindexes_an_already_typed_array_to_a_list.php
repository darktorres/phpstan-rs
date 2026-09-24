<?php
            /** @param array<string, int> $x
             *  @return list<int>
             */
            function f(array $x): array {
                sort($x);
                return $x;
            }
        