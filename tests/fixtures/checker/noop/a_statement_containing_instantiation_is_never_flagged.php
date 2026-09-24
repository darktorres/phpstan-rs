<?php
            class Foo {}
            function test(bool $a): void {
                $a || new Foo();
            }
        