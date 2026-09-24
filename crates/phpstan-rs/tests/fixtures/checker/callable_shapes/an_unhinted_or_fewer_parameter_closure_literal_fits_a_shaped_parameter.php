<?php
            /** @param Closure(int, string): string $fn */
            function takesCallback($fn): void {}
            function test(): void {
                takesCallback(function ($x) { return 'a'; });
                takesCallback(fn () => 'b');
                takesCallback(fn (int $x, string $y): string => $y);
            }
        