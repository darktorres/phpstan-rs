<?php
            use function PHPStan\Testing\assertType;
            interface View {}
            class ClassMethod { public array $stmts = []; }
            function f(View $view): void {
                if ($view instanceof ClassMethod) {
                    assertType('array', $view->stmts);
                }
            }
        