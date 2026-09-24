<?php
            class Foo {}
            function test(int|Foo $k): array {
                return [$k => 1];
            }
        