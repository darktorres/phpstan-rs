<?php
            class TagId18 {
                private function __construct(public int $value) {}
                public function from(int $value): self { return new self($value); }
            }
            class OtherTagId18 {
                private function __construct(public int $value) {}
                public function from(int $value): self { return new self($value); }
            }
            /** @param list<int> $ids
             *  @return list<TagId18>
             */
            function f(TagId18|OtherTagId18 $first, array $ids): array {
                return array_map($first->from(...), $ids);
            }
        