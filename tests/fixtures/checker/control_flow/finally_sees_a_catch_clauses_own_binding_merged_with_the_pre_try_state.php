<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(): void {
                $x = null;
                try {
                    mightThrowOrReturn();
                    $x = new Foo();
                } catch (\Exception $e) {
                } finally {
                    assertType('null|Foo', $x);
                }
            }
            function mightThrowOrReturn(): void {}
        