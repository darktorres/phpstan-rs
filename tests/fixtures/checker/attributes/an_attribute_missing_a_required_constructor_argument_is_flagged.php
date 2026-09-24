<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b) {}
            }

            #[MyAttr(1)]
            class Foo {}
        