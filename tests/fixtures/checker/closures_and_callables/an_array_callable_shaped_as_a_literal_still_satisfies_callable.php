<?php
            class Handler { public function handle(int $x): int { return $x; } }
            function apply(callable $fn, int $x): int { return $fn($x); }
            function test(Handler $h): int { return apply([$h, 'handle'], 5); }
        