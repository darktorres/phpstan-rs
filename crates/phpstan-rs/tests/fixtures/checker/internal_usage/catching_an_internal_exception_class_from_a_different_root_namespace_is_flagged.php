<?php
            namespace Vendor\Internal;
            /** @internal */
            class ThingException extends \Exception {
            }

            namespace Other\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\ThingException $e) {
                }
            }
        