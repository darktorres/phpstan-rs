<?php
            class Point {
                public function __construct(int $x, int $y) {}
            }
            function test(array $coords): Point { return new Point(...$coords); }
        