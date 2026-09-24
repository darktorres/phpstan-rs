<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                $method = $reflClass->getMethod('bar');
                assertType('ReflectionClass<Foo>', $method->getDeclaringClass());
            }
        