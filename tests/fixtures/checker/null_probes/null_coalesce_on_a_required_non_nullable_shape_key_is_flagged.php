<?php
            /** @param array{key: int} $x */
            function test(array $x): int {
                return $x['key'] ?? 0;
            }
        