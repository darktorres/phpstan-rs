<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            $f = function (): \Vendor\Internal\Foo {
                throw new \Exception();
            };
        