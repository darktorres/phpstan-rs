<?php
            class Base {
                public function __construct(private final int $x = 1) {}
            }
            class Child extends Base {
                public function __construct(public int $x = 2) {}
            }
        