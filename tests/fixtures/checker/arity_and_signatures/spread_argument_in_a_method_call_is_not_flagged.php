<?php
            class Calc { public function add(int $a, int $b): int { return $a + $b; } }
            function test(Calc $c, array $args): int { return $c->add(...$args); }
        