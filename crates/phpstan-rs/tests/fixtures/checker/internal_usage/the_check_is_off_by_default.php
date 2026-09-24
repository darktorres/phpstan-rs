<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Vendor\App;
            function f() {
                \Vendor\Internal\make();
            }
        