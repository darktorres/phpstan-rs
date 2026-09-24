<?php
            function f(int $width, int $mid, int $height): void {}
            function g(int $height, int $mid, int $width): void {
                f($height, $mid, $width);
            }
        