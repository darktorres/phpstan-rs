<?php
            function needsInt(int $n): void {}
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    needsInt($x['a']);
                }
            }
        