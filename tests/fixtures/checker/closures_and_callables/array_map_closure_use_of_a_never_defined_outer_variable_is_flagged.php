<?php
            /** @param int[] $items */
            function test(array $items): array {
                return array_map(function (int $x) use ($typo) {
                    return $x + $typo;
                }, $items);
            }
        