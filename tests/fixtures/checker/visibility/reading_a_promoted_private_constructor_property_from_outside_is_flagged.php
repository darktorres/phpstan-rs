<?php
            class Foo {
                public function __construct(private int $x) {}
            }
            function test(Foo $f): int {
                return $f->x;
            }
        