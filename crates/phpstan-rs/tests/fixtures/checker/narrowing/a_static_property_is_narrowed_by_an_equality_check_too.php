<?php
            class Cache1 {
                private static ?string $value = null;
                public static function get(): string {
                    if (self::$value !== null) {
                        return self::$value;
                    }
                    return self::$value = 'computed';
                }
            }
        