<?php
            /** @param array{status?: int|null} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? null)) {
                    return $decoded['status'];
                }
                return 0;
            }
        