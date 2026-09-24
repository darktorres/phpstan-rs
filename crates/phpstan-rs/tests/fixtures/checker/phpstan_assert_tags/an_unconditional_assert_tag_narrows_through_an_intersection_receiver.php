<?php
            interface View {}
            class Assertions {
                /** @phpstan-assert string $value */
                public function string($value): void {
                    if (!is_string($value)) {
                        throw new \RuntimeException('not a string');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(View $view, mixed $x): void {
                if ($view instanceof Assertions) {
                    $view->string($x);
                    needsString($x);
                }
            }
        