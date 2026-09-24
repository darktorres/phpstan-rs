<?php
            namespace Vendor\Internal;
            class Thing {
                /** @internal */
                public function doStuff(): void {}
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                $t->doStuff();
            }
        