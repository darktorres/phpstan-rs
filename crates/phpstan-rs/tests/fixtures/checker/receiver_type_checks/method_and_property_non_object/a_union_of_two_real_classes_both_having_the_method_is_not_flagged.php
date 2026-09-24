<?php
                class Foo {
                    function bar() {}
                }
                class Baz {
                    function bar() {}
                }
                /** @param Foo|Baz $x */
                function f($x) {
                    $x->bar();
                }
            