<?php
            function needsInt(int $x): void {}
            /** @param array<string, mixed> $row */
            function f(array $row): void {
                foreach (['id', 'name'] as $key) {
                    $row[$key] = is_numeric($row[$key] ?? null) ? (int) ($row[$key] ?? 0) : 0;
                }
                needsInt($row['id']);
            }
        