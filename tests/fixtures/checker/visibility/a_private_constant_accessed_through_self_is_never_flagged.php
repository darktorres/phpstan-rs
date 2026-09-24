<?php
            class Foo {
                private const X = 1;
                public static function get(): int {
                    return self::X;
                }
            }
        