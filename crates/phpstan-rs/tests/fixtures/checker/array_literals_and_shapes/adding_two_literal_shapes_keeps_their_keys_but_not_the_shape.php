<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = ['a' => 1];
                $b = ['b' => 'x'];
                // PHP gives array{a: 1, b: 'x'}; the union keeps each key
                // and value but not which value goes with which key.
                assertType("non-empty-array<'a'|'b', 1|'x'>", $a + $b);
            }
        