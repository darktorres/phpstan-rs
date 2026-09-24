<?php
            class Calc40 { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc40 $c): int { return $c->add(1, 2, 3, 4); }
        