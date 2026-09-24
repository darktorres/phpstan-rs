<?php
            use function PHPStan\Testing\assertType;
            final class Foo {
                public int $bar = 1;
            }
            function f(): void {
                $name = 'bar';
                $reflClass = new ReflectionClass(Foo::class);
                $prop = $reflClass->getProperty($name);
                assertType('int', $prop->getValue(new Foo()));
            }
        