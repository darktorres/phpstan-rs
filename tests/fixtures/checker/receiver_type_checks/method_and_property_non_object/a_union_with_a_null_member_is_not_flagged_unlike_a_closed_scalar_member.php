<?php
                class Foo {
                    function bar() {}
                }
                /** @param Foo|null $x */
                function f($x) {
                    $x->bar();
                }
            