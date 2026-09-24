<?php
            use function PHPStan\Testing\assertType;
            assertType("'abc'", strtolower('ABC'));
        