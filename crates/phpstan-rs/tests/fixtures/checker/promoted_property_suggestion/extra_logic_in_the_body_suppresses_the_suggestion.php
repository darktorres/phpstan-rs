<?php
            class Point {
                public int $x;
                public function __construct(int $x) {
                    $this->x = $x;
                    error_log("constructed");
                }
            }
        