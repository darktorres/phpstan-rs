<?php
            define('true', (bool) 1, true);
            define('false', (bool) 0, true);
            define('null', null, true);
            function findOrFalse(): int|false { return \false; }
            function run(): void {
                $x = findOrFalse();
                if ($x !== \false) {
                    needsInt($x);
                }
            }
            function needsInt(int $x): void {}
        