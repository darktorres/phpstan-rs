<?php
            interface View { public function value(): int; }
            class HasValue {
                private function value(): int { return 1; }
                public function useIt(): int { return $this->value(); }
            }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    $view->value();
                }
            }
        