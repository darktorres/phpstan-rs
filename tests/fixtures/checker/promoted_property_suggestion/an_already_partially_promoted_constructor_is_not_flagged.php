<?php
            class Point {
                public int $y;
                public function __construct(public int $x, int $y) {
                    $this->y = $y;
                }
            }
        