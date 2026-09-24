<?php
            function distance(float $x, float $y): float { return $x + $y; }
            function test(): float { return distance(3, 4); }
            class Config { public float $rate = 0; }
            function test2(Config $c): void { $c->rate = 5; }
        