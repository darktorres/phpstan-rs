<?php
            class Foo {
                const BAR = 1;
                public static int $counter = 0;
                public static function baz(): int { return 1; }
            }
            function f() {
                echo Foo::BAR;
                echo Foo::$counter;
                echo Foo::baz();
                echo Foo::class;
                echo Foo::TOTALLY_MADE_UP;
                echo Foo::$madeUpProp;
                echo Foo::madeUpMethod();
            }
        