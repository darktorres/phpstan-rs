<?php
            use function PHPStan\Testing\assertType;
            final class Foo {}
            /**
             * @template T of object
             * @param class-string<T> $class
             * @return T|null
             */
            function make(string $class): object|null {
                return null;
            }
            assertType('Foo|null', make('Foo'));
        