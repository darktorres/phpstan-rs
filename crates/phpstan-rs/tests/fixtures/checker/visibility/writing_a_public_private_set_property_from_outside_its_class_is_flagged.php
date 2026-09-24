<?php
            class Foo {
                public private(set) int $x = 1;
            }
            function test(Foo $f): void {
                $f->x = 2;
            }
        