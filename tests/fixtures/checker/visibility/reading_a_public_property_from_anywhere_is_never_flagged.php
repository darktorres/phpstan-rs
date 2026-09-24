<?php
            class Foo {
                public int $x = 1;
            }
            function test(Foo $f): int {
                return $f->x;
            }
        