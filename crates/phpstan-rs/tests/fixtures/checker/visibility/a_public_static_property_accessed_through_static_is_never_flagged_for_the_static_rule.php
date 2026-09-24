<?php
            class Foo {
                public static int $x = 1;
                public static function get(): int {
                    return static::$x;
                }
            }
        