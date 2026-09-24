<?php
            use function PHPStan\Testing\assertType;
            function f(): \Generator {
                assertType('mixed', yield from [1, 2, 3]);
            }
        