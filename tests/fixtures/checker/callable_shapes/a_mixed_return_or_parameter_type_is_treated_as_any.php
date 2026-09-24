<?php
            /** @param Closure(int): string $fn */
            function needsString($fn): void {}
            /** @param Closure(mixed): mixed $fn */
            function test($fn): void {
                needsString($fn);
            }
        