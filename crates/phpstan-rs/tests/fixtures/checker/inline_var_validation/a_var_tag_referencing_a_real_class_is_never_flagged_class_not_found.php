<?php
            class Foo {}
            function f(): void {
                /** @var Foo $x */
                $x = new Foo();
            }
        