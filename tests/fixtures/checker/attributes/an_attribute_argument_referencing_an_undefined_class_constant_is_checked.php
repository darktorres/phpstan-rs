<?php
            class Real {}

            #[Attribute]
            class MyAttr {
                public function __construct(public $a) {}
            }

            #[MyAttr(Real::UNDEFINED_CONST)]
            class Foo {}
        