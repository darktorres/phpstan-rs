<?php
            interface View {}
            class RealException extends \Exception {}
            function test(View $view): void {
                if ($view instanceof RealException) {
                    throw $view;
                }
            }
        