<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
            }

            namespace Other\App;
            function f(object $o) {
                $o instanceof \Vendor\Internal\Thing;
            }
        