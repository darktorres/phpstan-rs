<?php
            class Foo {
                private static int $x = 1;
                public static function get(): int {
                    return static::$x;
                }
            }
        