<?php
            class Base {
                private const X = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return self::X;
                }
            }
        