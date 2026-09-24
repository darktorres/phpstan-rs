<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    } else {
                        $this->x = 2;
                    }
                    echo $this->x;
                }
            }
        