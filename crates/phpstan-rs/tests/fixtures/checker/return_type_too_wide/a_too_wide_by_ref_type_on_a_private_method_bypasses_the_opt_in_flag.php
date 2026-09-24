<?php
            class Foo {
                private function f(int|string &$x): void {
                    $x = 1;
                }
            }
        