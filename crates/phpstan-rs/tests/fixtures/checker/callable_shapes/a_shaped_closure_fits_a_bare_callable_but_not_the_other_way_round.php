<?php
            /** @param Closure(int): string $fn */
            function needsShaped($fn): void {}
            /** @param callable $fn */
            function needsBare($fn): void {}
            /** @param Closure(int): string $fn */
            function test($fn): void {
                needsBare($fn);
            }
            function test2(callable $fn): void {
                needsShaped($fn);
            }
        