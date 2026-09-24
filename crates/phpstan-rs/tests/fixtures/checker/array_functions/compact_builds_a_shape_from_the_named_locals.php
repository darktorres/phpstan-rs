<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = 1;
                $b = 'x';
                assertType("array{a: 1, b: 'x'}", compact('a', 'b'));
            }
        