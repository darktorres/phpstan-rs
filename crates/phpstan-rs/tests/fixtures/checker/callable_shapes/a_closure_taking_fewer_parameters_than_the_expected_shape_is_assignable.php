<?php
            /** @param Closure(int, string): void $fn */
            function needsTwoArgs($fn): void {}
            /** @param Closure(int): void $fn */
            function test($fn): void {
                needsTwoArgs($fn);
            }
        