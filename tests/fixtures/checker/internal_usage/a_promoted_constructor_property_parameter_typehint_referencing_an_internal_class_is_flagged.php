<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            class C {
                public function __construct(private \Vendor\Internal\Foo $foo) {}
            }
        