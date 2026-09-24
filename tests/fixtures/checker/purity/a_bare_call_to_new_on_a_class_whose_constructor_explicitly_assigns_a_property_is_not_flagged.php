<?php
            final class Foo {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                }
            }
            new Foo(1);
        