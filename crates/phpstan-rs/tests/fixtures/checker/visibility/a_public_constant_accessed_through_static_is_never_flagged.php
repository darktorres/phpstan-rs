<?php
            class Foo {
                public const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        