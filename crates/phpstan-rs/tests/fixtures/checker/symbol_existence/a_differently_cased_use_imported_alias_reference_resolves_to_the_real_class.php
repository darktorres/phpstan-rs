<?php
            namespace App;
            class Foo {}

            namespace Other;
            use App\Foo as Bar;
            function test(): void {
                new bar();
            }
        