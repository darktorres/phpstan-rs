<?php
                class Foo {
                    function bar() {}
                }
                /** @param Foo|string $x */
                function f($x) {
                    $x->bar();
                }
            