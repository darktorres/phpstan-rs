<?php
            class Foo {}
            /** @param Foo $obj */
            function f($obj) {
                return $obj::madeUpMethod();
            }
        