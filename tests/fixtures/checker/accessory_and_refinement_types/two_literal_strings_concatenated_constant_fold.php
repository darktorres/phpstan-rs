<?php
            use function PHPStan\Testing\assertType;
            assertType("'ab'", 'a' . 'b');
        