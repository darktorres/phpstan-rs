<?php
            class Foo {}
            /** @param int[] $arr */
            function test(array $arr, Foo $key) {
                return $arr[$key];
            }
        