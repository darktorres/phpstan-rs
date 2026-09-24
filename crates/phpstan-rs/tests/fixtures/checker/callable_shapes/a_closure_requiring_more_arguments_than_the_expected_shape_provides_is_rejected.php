<?php
            /** @param Closure(int): void $fn */
            function needsOneArg($fn): void {}
            /** @param Closure(int, int): void $fn */
            function test($fn): void {
                needsOneArg($fn);
            }
        