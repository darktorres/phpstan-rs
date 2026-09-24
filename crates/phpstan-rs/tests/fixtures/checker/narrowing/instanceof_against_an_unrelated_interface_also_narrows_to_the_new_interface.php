<?php
            interface View {}
            interface HasPageAssets { public function pageAssets(): array; }
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    $view->pageAssets();
                }
            }
        