<?php
            class Base {
                public static function make(): void {
                    self::make();
                    static::make();
                }
            }
        