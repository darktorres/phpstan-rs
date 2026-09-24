<?php
            class Base {
                public function __construct(public readonly int $x) {}
            }
            class Child extends Base {
                public int $x = 1;
            }
        