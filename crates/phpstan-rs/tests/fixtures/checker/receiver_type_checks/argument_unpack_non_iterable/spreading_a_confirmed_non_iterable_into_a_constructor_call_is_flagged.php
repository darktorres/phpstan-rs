<?php
                class Foo {
                    function __construct($a, $b) {}
                }
                function f(int $x) {
                    new Foo(...$x);
                }
            