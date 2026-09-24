<?php
            use function PHPStan\Testing\assertType;
            class A { public function b(): string { return "x"; } }
            function f(?A $a): void {
                if ($a?->b() !== null) {
                    assertType('A', $a);
                }
            }
        