<?php
            namespace App;
            class Foo {}

            namespace Other;
            use App\Foo;
            /** @param foo $x */
            function f($x): void {}
        