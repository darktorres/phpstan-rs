<?php
            class Multiplier { public function __invoke(int $x): int { return $x * 2; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(): int { return apply(new Multiplier(), 5); }
        