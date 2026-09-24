<?php
            class Foo {
                private function secret(): int { return 1; }
            }
            function test(Foo $f): int {
                return $f->secret();
            }
        