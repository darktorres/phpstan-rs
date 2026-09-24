<?php
            /** @param Closure(int $x): string $fn */
            function test($fn): void {
                $fn(x: 'not an int');
            }
        