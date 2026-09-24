<?php
            function f(int $a, string $b): void {}
            function test(): void {
                f(...['a' => 1, 'b' => 2]);
            }
        