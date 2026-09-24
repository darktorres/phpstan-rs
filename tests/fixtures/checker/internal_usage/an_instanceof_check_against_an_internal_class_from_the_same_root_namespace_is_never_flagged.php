<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Vendor\App;
            function f(object $o) {
                $o instanceof \Vendor\Internal\Thing;
            }
        