<?php
            namespace Vendor\Internal;
            class Thing {
                /** @internal */
                const X = 1;
            }

            namespace Vendor\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        