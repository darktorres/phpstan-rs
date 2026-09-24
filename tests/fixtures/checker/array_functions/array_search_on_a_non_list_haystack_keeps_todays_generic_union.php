<?php
            /** @param array<string, string> $haystack */
            function indexOf(array $haystack, string $needle): int {
                $index = array_search($needle, $haystack, true);
                if ($index === false) {
                    throw new RuntimeException('not found');
                }
                return $index;
            }
        