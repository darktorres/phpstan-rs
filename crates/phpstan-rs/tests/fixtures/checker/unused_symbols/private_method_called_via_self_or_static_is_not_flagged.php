<?php
            class Service {
                public function run(): int {
                    return self::helper() + static::other();
                }
                private static function helper(): int { return 1; }
                private static function other(): int { return 2; }
            }
        