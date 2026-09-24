<?php
            namespace Vendor\Internal;
            /** @internal */
            class NotAnException {
            }

            namespace Other\App;
            function f() {
                try {
                    doStuff();
                } catch (\Vendor\Internal\NotAnException $e) {
                }
            }
        