<?php
            class Foo {
                public int $x;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        