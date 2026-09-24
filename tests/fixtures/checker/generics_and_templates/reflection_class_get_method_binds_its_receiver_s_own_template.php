<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                assertType(
                    "ReflectionMethod<Foo>&reflectedMember(Foo, 'bar')",
                    $reflClass->getMethod('bar'),
                );
            }
        