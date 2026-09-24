<?php
            interface View {}
            class Foo {
                private int $secret = 1;
                public function useIt(): int { return $this->secret; }
            }
            function f(View $view): void {
                if ($view instanceof Foo) {
                    $view->secret = "not an int";
                }
            }
        