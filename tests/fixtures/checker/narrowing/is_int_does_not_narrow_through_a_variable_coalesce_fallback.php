<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded, int $default): int {
                if (is_int($decoded['status'] ?? $default)) {
                    return $decoded['status'];
                }
                return 0;
            }
        