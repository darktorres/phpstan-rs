<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('int|false', strtotime('not a real relative time'));
            }
        