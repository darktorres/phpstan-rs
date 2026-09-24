<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('float', 10 / 4);
            }
        