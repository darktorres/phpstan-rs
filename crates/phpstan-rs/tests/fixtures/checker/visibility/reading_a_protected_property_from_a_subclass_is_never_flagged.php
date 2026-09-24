<?php
            class Base {
                protected int $x = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return $this->x;
                }
            }
        