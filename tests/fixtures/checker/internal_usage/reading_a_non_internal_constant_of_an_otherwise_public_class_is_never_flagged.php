<?php
            namespace Vendor\Internal;
            class Thing {
                const X = 1;
            }

            namespace Other\App;
            function f() {
                echo \Vendor\Internal\Thing::X;
            }
        