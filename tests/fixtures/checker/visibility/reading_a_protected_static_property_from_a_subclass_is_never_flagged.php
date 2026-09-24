<?php
            class Base {
                protected static int $x = 1;
            }
            class Child extends Base {
                public static function get(): int {
                    return self::$x;
                }
            }
        