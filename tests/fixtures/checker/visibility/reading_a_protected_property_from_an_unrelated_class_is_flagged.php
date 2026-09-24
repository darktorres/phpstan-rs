<?php
            class Foo {
                protected int $x = 1;
            }
            class Unrelated {
                public function test(Foo $f): int {
                    return $f->x;
                }
            }
        