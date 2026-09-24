<?php
            class Foo {
                public int $x = 1;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        