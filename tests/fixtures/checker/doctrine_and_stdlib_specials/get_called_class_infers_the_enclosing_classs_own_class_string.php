<?php
            class Base {}
            class TypedRepository {
                /**
                 * @template R of Base
                 * @param class-string<R> $repositoryClass
                 * @return R
                 */
                public static function narrow(string $repositoryClass) {
                    return new $repositoryClass();
                }
            }
            class Derived extends Base {
                public static function make(): int {
                    return TypedRepository::narrow(get_called_class());
                }
            }
        