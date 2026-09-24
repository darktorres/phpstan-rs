<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? 0)) {
                    return $decoded['status'];
                }
                return 0;
            }
        