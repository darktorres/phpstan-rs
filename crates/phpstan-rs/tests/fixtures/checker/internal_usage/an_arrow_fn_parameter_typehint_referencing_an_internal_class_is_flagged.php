<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = fn(\Vendor\Internal\Foo $x) => null;
        