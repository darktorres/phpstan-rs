<?php
            class Foo {
                private static int $x = 1;
            }
            function test(): void {
                Foo::$x = 2;
            }
        