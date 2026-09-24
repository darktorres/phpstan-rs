<?php
            class Foo {}
            /**
             * @param int[]|string[] $arr
             */
            function test(array $arr, Foo $key) {
                return $arr[$key];
            }
        