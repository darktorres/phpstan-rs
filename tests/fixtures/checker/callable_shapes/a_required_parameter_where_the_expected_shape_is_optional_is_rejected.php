<?php
            /** @param Closure(int=): void $fn */
            function mayOmitArg($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                mayOmitArg($fn);
            }
        