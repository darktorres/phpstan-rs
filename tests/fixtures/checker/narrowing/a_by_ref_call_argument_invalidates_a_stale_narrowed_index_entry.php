<?php
            /**
             * @param array<string, mixed> $arr
             */
            function widenIt(array &$arr): void {}
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                if (is_int($arr['k'])) {
                    widenIt($arr);
                    needsString($arr['k']);
                }
            }
        