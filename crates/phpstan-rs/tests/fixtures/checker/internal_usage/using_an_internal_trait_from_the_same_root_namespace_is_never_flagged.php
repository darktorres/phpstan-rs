<?php
            namespace Vendor\Internal;
            /** @internal */
            trait T {}

            namespace Vendor\App;
            class C {
                use \Vendor\Internal\T;
            }
        