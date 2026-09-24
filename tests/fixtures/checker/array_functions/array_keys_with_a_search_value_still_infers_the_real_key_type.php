<?php
            /** @param array<string, string> $byLower */
            function f(array $byLower, string $needle): string {
                $found = array_keys($byLower, $needle, true);
                return $found[0];
            }
        