<?php
            /** @param list<string> $a
             *  @return non-empty-list<string>
             */
            function f($a): array {
                return array_merge(['x', 'y'], $a);
            }
        