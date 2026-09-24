<?php
            class Base {
                protected static function helper(): int { return 1; }
            }
            class Child extends Base {
                public static function reveal(): int {
                    return static::helper();
                }
            }
        