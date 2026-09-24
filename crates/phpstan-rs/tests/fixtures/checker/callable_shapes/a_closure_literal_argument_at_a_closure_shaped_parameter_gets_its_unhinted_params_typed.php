<?php
            /** @param Closure(int): string $fn */
            function apply($fn): void {}
            apply(function ($x) {
                return (string) ($x + 1);
            });
        