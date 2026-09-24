<?php
            class Point {
                public function __construct(public int $x) {}
            }
            $p = new Point(1);
            $p->x = "nope";
        