<?php
            class Foo {}
            /** @param int[] $arr */
            function test(array $arr, Foo $key): void {
                $arr[$key] = 1;
            }
        