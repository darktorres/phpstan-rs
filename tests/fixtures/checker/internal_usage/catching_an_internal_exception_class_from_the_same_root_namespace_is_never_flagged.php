<?php
            namespace Vendor\Internal;
            /** @internal */
            class ThingException extends \Exception {
            }

            namespace Vendor\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\ThingException $e) {
                }
            }
        