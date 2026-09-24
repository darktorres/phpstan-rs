<?php
            function apply(callable $fn, string $s): int { return $fn($s); }
            function test(): int { return apply('strlen', "hello"); }
        