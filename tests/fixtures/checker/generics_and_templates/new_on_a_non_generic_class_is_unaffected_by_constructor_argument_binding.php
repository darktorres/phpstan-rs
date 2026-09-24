<?php
            use function PHPStan\Testing\assertType;
            class Plain {
                public function __construct(public int $x) {}
            }
            assertType('Plain', new Plain(5));
        