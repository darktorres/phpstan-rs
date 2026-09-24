<?php
            /** @param list<int> $a
             *  @return non-empty-list<int>
             */
            function f($a): array {
                return [1, ...$a];
            }
        