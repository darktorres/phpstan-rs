<?php
            use function PHPStan\Testing\assertType;
            /** @template T */
            class Box {
                /** @param T $value */
                public function __construct(public $value) {}
            }
            assertType('Box<5>', new Box(5));
        