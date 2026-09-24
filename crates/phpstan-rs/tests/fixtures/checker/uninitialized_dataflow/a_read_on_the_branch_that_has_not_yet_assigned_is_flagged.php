<?php
            class Foo {
                public int $x;
                public function __construct(bool $c) {
                    if ($c) {
                        $this->x = 1;
                    } else {
                        echo $this->x;
                        $this->x = 2;
                    }
                }
            }
        