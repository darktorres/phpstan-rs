<?php
            class Foo {
                public static function bar($x) {}
            }
            function f() {
                Foo::bar($undefinedVar);
            }
        