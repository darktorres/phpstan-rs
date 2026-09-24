<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Vendor\App;
            function f(\Vendor\Internal\Foo $x): void {}
        