<?php
            function needsIntOrStringOrBool(int|string|bool $x): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                $x = $arr['x'] ?? null;
                $x = is_int($x) || is_string($x) || is_bool($x) ? $x : 0;
                needsIntOrStringOrBool($x);
            }
        