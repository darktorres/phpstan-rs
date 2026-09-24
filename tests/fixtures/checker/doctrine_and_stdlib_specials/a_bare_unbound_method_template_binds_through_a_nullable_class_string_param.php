<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            class Holder {
                /**
                 * Gets a typed value.
                 *
                 * @template T
                 *
                 * Returns the requested value.
                 *
                 * @param class-string<T>|null $name
                 * @return T
                 */
                public function get(?string $name = null) {}
            }
            function f(Holder $h): void {
                assertType('Foo', $h->get(Foo::class));
            }
        