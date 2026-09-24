<?php
            class Base {}
            class Mid extends Base {}
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
            class Bottom extends Mid {
                public static function make(): int {
                    return TypedRepository::narrow(get_parent_class(new self()));
                }
            }
        