<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(): void {
                $x = null;
                try {
                    $x = new Foo();
                } finally {
                    echo "cleanup";
                }
                assertType('Foo', $x);
            }
        