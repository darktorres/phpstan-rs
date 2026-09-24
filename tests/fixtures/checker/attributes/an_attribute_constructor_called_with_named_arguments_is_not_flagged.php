<?php
            #[Attribute]
            class MyAttr {
                public function __construct(public int $a, public string $b = "default") {}
            }

            #[MyAttr(a: 1)]
            class Foo {}
        