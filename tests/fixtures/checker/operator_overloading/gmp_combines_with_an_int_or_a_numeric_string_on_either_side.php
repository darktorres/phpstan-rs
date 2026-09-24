<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g, int $i): void {
                assertType('GMP', $g + $i);
                assertType('GMP', $i + $g);
                assertType('GMP', $g + '123');
            }
        