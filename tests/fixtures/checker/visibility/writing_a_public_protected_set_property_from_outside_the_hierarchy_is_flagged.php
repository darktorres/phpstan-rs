<?php
            class Foo {
                public protected(set) int $x = 1;
            }
            function test(Foo $f): void {
                $f->x = 2;
            }
        