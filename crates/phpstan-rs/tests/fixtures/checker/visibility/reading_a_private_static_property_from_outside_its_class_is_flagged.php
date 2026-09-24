<?php
            class Foo {
                private static int $x = 1;
            }
            function test(): int {
                return Foo::$x;
            }
        