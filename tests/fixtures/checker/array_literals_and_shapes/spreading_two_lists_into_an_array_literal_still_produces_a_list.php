<?php
            /** @param list<int> $a
             *  @param list<int> $b
             *  @return list<int>
             */
            function f($a, $b): array {
                return [...$a, ...$b];
            }
        