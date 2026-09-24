<?php
            namespace Vendor\Internal;
            /** @internal */
            class Thing {
                public function __toString(): string { return "x"; }
            }

            namespace Other\App;
            function f(\Vendor\Internal\Thing $t) {
                echo (string) $t;
            }
        