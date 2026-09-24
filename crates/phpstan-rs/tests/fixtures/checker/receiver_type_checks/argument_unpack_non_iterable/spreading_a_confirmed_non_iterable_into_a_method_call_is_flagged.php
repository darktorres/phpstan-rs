<?php
                class Foo {
                    function g($a, $b) {}
                }
                function f(Foo $obj, int $x) {
                    $obj->g(...$x);
                }
            