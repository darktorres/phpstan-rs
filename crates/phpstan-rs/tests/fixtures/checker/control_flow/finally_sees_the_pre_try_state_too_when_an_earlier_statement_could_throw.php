<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function mightThrow(): void {}
            function f(): void {
                $x = null;
                try {
                    mightThrow();
                    $x = new Foo();
                } finally {
                    assertType('null|Foo', $x);
                }
            }
        