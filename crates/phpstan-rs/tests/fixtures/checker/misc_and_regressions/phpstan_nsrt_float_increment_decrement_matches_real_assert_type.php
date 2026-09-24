<?php
            use function PHPStan\Testing\assertType;
            function test(float $f): void { assertType('float', ++$f); }
            function test2(float $f): void { assertType('float', --$f); }
            function test3(float $f): void { assertType('float', $f++); }
            function test4(float $f): void { assertType('float', $f--); }
        