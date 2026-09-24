<?php
            class Foo {}
            class Bar {}
            function test(Foo|Bar $x): string {
                return (string) $x;
            }
        