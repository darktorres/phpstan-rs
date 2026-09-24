<?php
            namespace Vendor\Internal;
            /** @internal */
            interface Thing {
                public function doStuff(): void;
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        