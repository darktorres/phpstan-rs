<?php
            define('__DIR__', '', true);
            define('__LINE__', 0, true);
            /** @param non-empty-string $x */
            function needsNonEmptyString($x): void {}
            function run(): void {
                needsNonEmptyString(__DIR__);
            }
        