<?php
            /** @param Closure(int): int $fn */
            function needsIntReturn($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsIntReturn($fn);
            }
        