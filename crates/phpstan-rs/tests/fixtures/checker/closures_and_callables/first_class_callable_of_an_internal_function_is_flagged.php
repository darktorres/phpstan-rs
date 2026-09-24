<?php
            namespace Vendor\Internal;
            /** @internal */
            function make(): void {}

            namespace Other\App;
            function test(): void {
                $f = \Vendor\Internal\make(...);
            }
        