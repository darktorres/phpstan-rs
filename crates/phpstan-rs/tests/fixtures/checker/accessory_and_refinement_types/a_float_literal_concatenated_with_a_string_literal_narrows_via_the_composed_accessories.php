<?php
            use function PHPStan\Testing\assertType;
            assertType("'1.5-suffix'", 1.5 . '-suffix');
        