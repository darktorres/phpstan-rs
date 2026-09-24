<?php
            abstract class Base {
                abstract protected function f(int|string &$x): void;
            }
            class Foo extends Base {
                protected function f(int|string &$x): void {
                    $x = 1;
                }
            }
        