<?php
            function f(int $a, string $b): void {}
            function test(): void {
                f(...[1, 2]);
            }
        