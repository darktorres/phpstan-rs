<?php
            class Foo {
                private static function secret(): int { return 1; }
                public static function get(): int {
                    return static::secret();
                }
            }
        