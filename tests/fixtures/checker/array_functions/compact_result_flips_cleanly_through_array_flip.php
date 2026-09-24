<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = 1;
                $b = 2;
                assertType("array{1: 'a', 2: 'b'}", array_flip(compact('a', 'b')));
            }
        