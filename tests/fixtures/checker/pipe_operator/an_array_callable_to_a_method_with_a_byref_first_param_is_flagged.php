<?php
            class Foo {
                public function takesByRef(&$y) { return $y; }
            }
            function test(int $x, Foo $f) {
                $x |> [$f, 'takesByRef'];
            }
        