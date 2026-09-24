<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    }
                }
            }
        