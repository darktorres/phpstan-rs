<?php
            use function PHPStan\Testing\assertType;
            interface View { public function value(): int|string; }
            class HasValue { public function value(): int { return 1; } }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    assertType('int', $view->value());
                }
            }
        