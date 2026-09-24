<?php
            use function PHPStan\Testing\assertType;
            /** @template T */
            class Box {
                public function __construct() {}
            }
            assertType('Box', new Box());
        