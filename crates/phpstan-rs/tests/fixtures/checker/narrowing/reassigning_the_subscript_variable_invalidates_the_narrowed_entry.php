<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key, string $other): void {
                if (is_int($arr[$key])) {
                    $key = $other;
                    needsString($arr[$key]);
                }
            }
        