<?php
            class Foo {
                private int $x = 1;
            }
            function test(Foo $f): int {
                return $f->x;
            }
        