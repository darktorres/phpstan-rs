<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            /** @param \Vendor\Internal\Foo $x */
            function f($x): void {}
        