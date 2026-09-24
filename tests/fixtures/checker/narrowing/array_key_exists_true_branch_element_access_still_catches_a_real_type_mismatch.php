<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    needsString($x['a']);
                }
            }
        