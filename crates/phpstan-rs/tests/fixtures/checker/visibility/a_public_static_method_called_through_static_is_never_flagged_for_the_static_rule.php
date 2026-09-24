<?php
            class Foo {
                public static function secret(): int { return 1; }
                public static function get(): int {
                    return static::secret();
                }
            }
        