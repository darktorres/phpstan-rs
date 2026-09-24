<?php
            use function PHPStan\Testing\assertType;
            function test(int $x): void { assertType('string', $x); }
        