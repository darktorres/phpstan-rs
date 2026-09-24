<?php
            use function PHPStan\Testing\assertType;
            function f(): \Generator {
                assertType('mixed', yield 'x');
            }
        