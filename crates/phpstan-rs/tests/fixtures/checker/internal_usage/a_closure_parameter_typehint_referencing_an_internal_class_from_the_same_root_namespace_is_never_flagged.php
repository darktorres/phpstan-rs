<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Vendor\App;
            $f = function (\Vendor\Internal\Foo $x): void {};
        