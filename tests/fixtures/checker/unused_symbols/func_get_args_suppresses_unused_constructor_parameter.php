<?php
            class Bag {
                public function __construct(int $x, int $y) {
                    $args = func_get_args();
                    echo count($args);
                }
            }
        