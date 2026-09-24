<?php
            class Foo {}
            function test(float $x, ?Foo $f): void {
                $x > $f;
            }
        