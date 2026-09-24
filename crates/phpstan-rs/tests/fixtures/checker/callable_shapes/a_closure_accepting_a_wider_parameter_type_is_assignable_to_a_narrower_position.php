<?php
            /** @param Closure(int): void $fn */
            function needsIntParam($fn): void {}
            /** @param Closure(int|string): void $fn */
            function test($fn): void {
                needsIntParam($fn);
            }
        