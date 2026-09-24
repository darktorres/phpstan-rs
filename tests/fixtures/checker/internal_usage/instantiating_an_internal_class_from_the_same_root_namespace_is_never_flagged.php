<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Vendor\App;
            function f() {
                new \Vendor\Internal\Thing();
            }
        