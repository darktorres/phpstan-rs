<?php
            use function PHPStan\Testing\assertType;
            class Box { public string|int $value = 0; }
            function f(Box $a, bool $cond): void {
                if ($cond) {
                    if (!is_string($a->value)) {
                        return;
                    }
                } else {
                    if (!is_string($a->value)) {
                        return;
                    }
                }
                assertType('string', $a->value);
            }
        