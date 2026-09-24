<?php
            function f(int $width, string $height): void {}
            function g(string $height, int $width): void {
                f($height, $width);
            }
        