<?php
            class Foo {
                public int $x;
                public function __construct(int $c) {
                    switch ($c) {
                        case 1:
                            $this->x = 1;
                            break;
                    }
                }
            }
        