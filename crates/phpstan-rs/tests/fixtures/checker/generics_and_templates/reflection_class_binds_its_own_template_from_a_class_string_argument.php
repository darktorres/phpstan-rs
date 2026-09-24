<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            assertType('ReflectionClass<Foo>', new ReflectionClass(Foo::class));
        