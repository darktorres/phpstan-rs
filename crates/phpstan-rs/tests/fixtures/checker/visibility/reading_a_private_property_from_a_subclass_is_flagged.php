<?php
            class Base {
                private int $x = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return $this->x;
                }
            }
        