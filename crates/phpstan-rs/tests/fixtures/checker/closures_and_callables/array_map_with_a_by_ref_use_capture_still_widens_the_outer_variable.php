<?php
            /** @param int[] $items */
            function test(array $items): int {
                $seen = [];
                array_map(function ($x) use (&$seen) {
                    $seen[] = $x;
                    return $x;
                }, $items);
                return count($seen);
            }
        