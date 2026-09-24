<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Other\App;
            function f() {
                new \Vendor\Internal\Thing();
            }
        