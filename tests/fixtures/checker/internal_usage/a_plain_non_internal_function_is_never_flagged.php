<?php
            namespace Vendor\Internal;
            function make(): void {}

            namespace Other\App;
            function f() {
                \Vendor\Internal\make();
            }
        