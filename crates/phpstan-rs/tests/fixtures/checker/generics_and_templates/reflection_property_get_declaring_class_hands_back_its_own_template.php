<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                $prop = $reflClass->getProperty('bar');
                assertType('ReflectionClass<Foo>', $prop->getDeclaringClass());
            }
        