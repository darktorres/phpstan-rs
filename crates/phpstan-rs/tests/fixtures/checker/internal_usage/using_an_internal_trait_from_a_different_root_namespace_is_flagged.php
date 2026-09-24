<?php
            namespace Vendor\Internal;
            /** @internal */
            trait T {}

            namespace Other\App;
            class C {
                use \Vendor\Internal\T;
            }
        