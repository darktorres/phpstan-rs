<?php
            #[Attribute]
            class MyAttr {
                private function __construct() {}
            }

            #[MyAttr]
            class Foo {}
        