<?php
            use function PHPStan\Testing\assertType;
            class A { public ?string $b = null; }
            function f(?A $a): void {
                $x = $a?->b;
                if ($x !== null) {
                    assertType('A', $a);
                }
            }
        