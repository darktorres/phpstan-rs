<?php
            class Foo {}
            class Bar {
                public function baz(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(fn(Bar $x) => $x->baz(), $items);
            }
        