<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public static function make(): void {}
            }

            namespace Other\App;
            function f() {
                $g = \Vendor\Internal\Thing::make(...);
            }
        