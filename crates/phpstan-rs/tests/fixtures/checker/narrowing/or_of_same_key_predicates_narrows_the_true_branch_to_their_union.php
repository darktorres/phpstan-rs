<?php
            function needsIntOrString(int|string $x): void {}
            /**
             * @param array<string, mixed> $chronology_date
             */
            function handle(array $chronology_date): void {
                $year = $chronology_date['year'] ?? null;
                $year = is_int($year) || is_string($year) ? $year : 0;
                needsIntOrString($year);
            }
        