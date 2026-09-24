<?php
            class Point {
                public function __construct(readonly int $x, final int $y) {
                    echo 1;
                }
            }
        