<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Other\App;
            function f() {
                \Vendor\Internal\make();
            }
        