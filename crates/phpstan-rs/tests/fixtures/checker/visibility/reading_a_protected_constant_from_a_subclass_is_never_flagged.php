<?php
            class Base {
                protected const X = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return self::X;
                }
            }
        