<?php
            class Foo {
                public ?int $x = null;
            }
            function test(Foo $f): void {
                if (isset($f->x)) {
                }
            }
        