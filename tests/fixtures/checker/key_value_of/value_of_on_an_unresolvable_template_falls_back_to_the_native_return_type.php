<?php
            use function PHPStan\Testing\assertType;
            /**
             * @template T of \UnitEnum
             */
            class Coercer {
                /**
                 * @return value-of<T>
                 */
                public function coerce(mixed $value): string|int {
                    return $value;
                }
            }
            function f(Coercer $c): void {
                assertType('string|int', $c->coerce('x'));
            }
        