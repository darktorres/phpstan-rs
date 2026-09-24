<?php
            interface View {}
            interface HasPageAssets {}
            function needsView(View $v): void {}
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    // no-op
                }
                needsView($view);
            }
        