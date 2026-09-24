<?php
            class Base {
                const SHARED = 1;
                public static function helper(): int { return 1; }
            }
            class Child extends Base {
                public function useParent(): int {
                    return parent::helper() + self::SHARED + static::helper();
                }
                public function useMadeUp(): int {
                    return parent::madeUpParentMethod();
                }
            }
        