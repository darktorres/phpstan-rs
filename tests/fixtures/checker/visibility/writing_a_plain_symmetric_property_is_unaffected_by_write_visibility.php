<?php
            class Foo {
                private int $x = 1;
            }
            function test(Foo $f): void {
                $f->x = 2;
            }
        