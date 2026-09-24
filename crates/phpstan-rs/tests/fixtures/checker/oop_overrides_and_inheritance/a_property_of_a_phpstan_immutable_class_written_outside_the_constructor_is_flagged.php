<?php
            /** @phpstan-immutable */
            class Point {
                public int $x;
                public function __construct(int $x) { $this->x = $x; }
                public function setX(int $x): void { $this->x = $x; }
            }
        