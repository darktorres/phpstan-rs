<?php
            use function PHPStan\Testing\assertType;
            assertType('non-falsy-string', 1.5 . '-suffix');
        