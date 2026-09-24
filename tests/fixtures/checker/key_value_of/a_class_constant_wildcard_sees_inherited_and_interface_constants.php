<?php
            use function PHPStan\Testing\assertType;
            interface HasCodes { const CODE_I = 'i'; }
            class Base { const CODE_A = 'a'; const CODE_B = 'b'; }
            class Child extends Base implements HasCodes {
                const CODE_B = 'overridden';
                const CODE_C = 'c';
                /** @param self::CODE_* $code */
                public function f($code): void {
                    assertType("'overridden'|'c'|'a'|'i'", $code);
                }
            }
            /** @param Child::CODE_* $code */
            function g($code): void { assertType("'overridden'|'c'|'a'|'i'", $code); }
        