<?php
            /** @param list<int> $a
             *  @param array<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        