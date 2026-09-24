<?php
            use function PHPStan\Testing\assertType;
            final class Foo {
                public function bar(): string { return 'x'; }
            }
            function f(): void {
                $reflClass = new ReflectionClass(Foo::class);
                $method = $reflClass->getMethod('bar');
                assertType('string', $method->invoke(new Foo()));
            }
        