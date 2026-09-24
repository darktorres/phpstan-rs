<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        