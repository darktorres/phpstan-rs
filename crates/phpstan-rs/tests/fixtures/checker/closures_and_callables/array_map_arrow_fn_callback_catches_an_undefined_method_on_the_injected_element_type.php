<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            /** @param Foo[] $items */
            function test(array $items) {
                return array_map(fn($x) => $x->undefinedMethod(), $items);
            }
        