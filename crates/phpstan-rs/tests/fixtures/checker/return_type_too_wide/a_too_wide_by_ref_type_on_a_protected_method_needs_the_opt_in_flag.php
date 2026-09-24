<?php
            class Foo {
                protected function f(int|string &$x): void {
                    $x = 1;
                }
            }
        