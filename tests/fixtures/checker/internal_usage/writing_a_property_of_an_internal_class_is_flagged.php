<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public int $x = 1;
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->x = 2;
            }
        