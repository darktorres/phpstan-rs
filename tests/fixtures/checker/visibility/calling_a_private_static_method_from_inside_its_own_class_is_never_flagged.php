<?php
            class Foo {
                private static function secret(): int { return 1; }
                public static function reveal(): int {
                    return self::secret();
                }
            }
        