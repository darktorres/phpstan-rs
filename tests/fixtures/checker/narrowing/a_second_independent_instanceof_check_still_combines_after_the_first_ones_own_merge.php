<?php
            interface View2 {}
            interface HasPageAssets2 {}
            interface ExposesPageData2 { public function exposedPageData(): array; }
            function needsView(View2 $v): void {}
            function f(View2 $view): void {
                if ($view instanceof HasPageAssets2) {
                    // no-op
                }
                if ($view instanceof ExposesPageData2) {
                    $view->exposedPageData();
                }
                needsView($view);
            }
        