<?php
            /** @param Closure(int|string): void $fn */
            function needsWiderParam($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                needsWiderParam($fn);
            }
        