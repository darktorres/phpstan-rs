<?php
            interface View {}
            class C {
                #[NoDiscard]
                function m(): int { return 1; }
            }
            function test(View $view) {
                if ($view instanceof C) {
                    $view->m();
                }
            }
        