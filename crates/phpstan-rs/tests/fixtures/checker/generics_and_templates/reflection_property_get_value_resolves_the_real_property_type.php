<?php
            use function PHPStan\Testing\assertType;
            final class Foo {
                public int $bar = 1;
            }
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                $prop = $reflClass->getProperty('bar');
                assertType('int', $prop->getValue(new Foo()));
            }
        