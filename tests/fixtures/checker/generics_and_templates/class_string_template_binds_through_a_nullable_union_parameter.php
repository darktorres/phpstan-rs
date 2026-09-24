<?php
            class Base {}
            class Derived extends Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R>|null $repositoryClass
                 * @return R
                 */
                public static function narrow(?string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            function f(): int {
                return TypedRepository::narrow(Derived::class);
            }
        