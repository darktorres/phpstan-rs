<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key): void {
                if (is_int($arr[$key])) {
                    $unrelated = 5;
                    needsString($arr[$key]);
                }
            }
        