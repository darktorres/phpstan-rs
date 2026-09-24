<?php
            namespace Vendor\Internal;
            /** @internal */
            class Foo {}

            namespace Other\App;
            function f(?\Vendor\Internal\Foo $x): void {}
        