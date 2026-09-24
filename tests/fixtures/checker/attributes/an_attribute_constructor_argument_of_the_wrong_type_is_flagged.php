<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a) {}
            }

            #[MyAttr("not an int")]
            class Foo {}
        