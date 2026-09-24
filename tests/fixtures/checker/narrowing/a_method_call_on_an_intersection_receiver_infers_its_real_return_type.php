<?php
            use function PHPStan\Testing\assertType;
            interface View {}
            class HasValue { public function value(): int { return 1; } }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    assertType('int', $view->value());
                    $x = $view->value() + 1;
                    assertType('int', $x);
                }
            }
        