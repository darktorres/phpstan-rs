<?php
            /** @phpstan-immutable */
            class Point {
                public int $x;
                public function __construct(int $x) { $this->x = $x; }
            }
            class Point3D extends Point {
                public int $z;
                public function __construct(int $x, int $z) {
                    parent::__construct($x);
                    $this->z = $z;
                }
                public function setZ(int $z): void { $this->z = $z; }
            }
        