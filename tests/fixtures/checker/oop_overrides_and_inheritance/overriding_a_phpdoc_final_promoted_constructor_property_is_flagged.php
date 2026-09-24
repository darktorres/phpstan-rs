<?php
            class Base {
                public function __construct(
                    /** @final */
                    public int $x = 1,
                ) {}
            }
            class Child extends Base {
                public function __construct(public int $x = 2) {}
            }
        