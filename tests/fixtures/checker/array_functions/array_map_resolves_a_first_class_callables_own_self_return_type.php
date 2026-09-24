<?php
            class TagId17 {
                private function __construct(public int $value) {}
                public static function tryFrom(mixed $value): ?self {
                    return is_int($value) ? new self($value) : null;
                }
            }
            /** @param list<mixed> $values
             *  @return list<TagId17|null>
             */
            function f(array $values): array {
                return array_map(TagId17::tryFrom(...), $values);
            }
        