<?php
            /** @param Closure(int): string $fn */
            function test($fn): void {
                $fn('not an int');
            }
        