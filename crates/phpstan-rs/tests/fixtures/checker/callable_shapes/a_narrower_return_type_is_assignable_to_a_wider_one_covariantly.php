<?php
            /** @param Closure(int): (string|int) $fn */
            function needsWiderReturn($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsWiderReturn($fn);
            }
        