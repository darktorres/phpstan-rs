<?php
            interface View {}
            class Foo {}
            function test(View $view): int {
                if ($view instanceof Foo) {
                    return (int) $view;
                }
                return 0;
            }
        