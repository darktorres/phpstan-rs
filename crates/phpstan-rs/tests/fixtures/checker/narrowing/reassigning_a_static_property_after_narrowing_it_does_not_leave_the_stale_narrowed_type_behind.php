<?php
            class Cache2 {
                private static ?string $value = null;
                public static function reset(): int {
                    if (self::$value !== null) {
                        self::$value = null;
                        return strlen(self::$value);
                    }
                    return 0;
                }
            }
        