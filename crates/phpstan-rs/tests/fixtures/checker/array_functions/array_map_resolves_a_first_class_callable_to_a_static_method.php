<?php
            class TagId16 {
                private function __construct(public int $value) {}
                public static function from(int $value): self { return new self($value); }
            }
            /** @param list<int> $ids
             *  @return list<TagId16>
             */
            function f(array $ids): array {
                return array_map(TagId16::from(...), $ids);
            }
        