<?php
            class Foo {
                protected const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        