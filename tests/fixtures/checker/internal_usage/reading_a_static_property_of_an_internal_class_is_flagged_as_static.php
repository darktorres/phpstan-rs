<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static int $x = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::$x;
            }
        