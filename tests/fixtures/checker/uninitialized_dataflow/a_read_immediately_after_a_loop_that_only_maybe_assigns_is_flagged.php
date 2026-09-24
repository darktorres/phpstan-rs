<?php
            class Foo {
                public int $x;
                public function __construct(array $items) {
                    foreach ($items as $item) {
                        $this->x = $item;
                    }
                    echo $this->x;
                }
            }
        