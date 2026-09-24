<?php
            class Base { public int $x = 1; }
            class Child extends Base {
                public function __construct(public readonly int $x) {}
            }
        