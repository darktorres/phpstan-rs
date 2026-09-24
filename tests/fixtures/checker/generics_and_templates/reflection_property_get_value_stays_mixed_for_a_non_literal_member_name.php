<?php
            use function PHPStan\Testing\assertType;
            final class Foo {
                public int $bar = 1;
            }
            function f(string $propName): void {
                $reflClass = new ReflectionClass(Foo::class);
                $prop = $reflClass->getProperty($propName);
                assertType('mixed', $prop->getValue(new Foo()));
            }
        