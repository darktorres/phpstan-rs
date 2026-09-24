<?php
            class Base {
                public function __construct(public final int $x = 1) {}
            }
            class Child extends Base {
                public function __construct(public int $x = 2) {}
            }
        