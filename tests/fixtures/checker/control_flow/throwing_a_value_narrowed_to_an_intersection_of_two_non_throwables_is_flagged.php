<?php
            interface View {}
            class NotThrowable {}
            function test(View $view): void {
                if ($view instanceof NotThrowable) {
                    throw $view;
                }
            }
        