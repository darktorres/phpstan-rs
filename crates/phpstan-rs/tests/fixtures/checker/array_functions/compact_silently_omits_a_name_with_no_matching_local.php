<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = 1;
                assertType('array{a: 1}', compact('a', 'undefinedName'));
            }
        