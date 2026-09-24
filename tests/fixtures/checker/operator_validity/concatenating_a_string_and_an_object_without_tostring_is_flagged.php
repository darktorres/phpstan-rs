<?php
            class Foo {}
            function test(string $s, Foo $f): void {
                $s . $f;
            }
        