<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static function make(): void {}
            }

            namespace Other\App;
            function f() {
                \Vendor\Internal\Thing::make();
            }
        