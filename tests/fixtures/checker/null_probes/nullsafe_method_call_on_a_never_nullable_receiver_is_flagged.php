<?php
            class Foo {
                public function bar(): int { return 1; }
            }
            function test(Foo $f): ?int {
                return $f?->bar();
            }
        