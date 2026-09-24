<?php
            use function PHPStan\Testing\assertType;
            function f($a, $b): void {
                /**
                 * @var int $a
                 * @var string $b
                 */
                echo 1;
                assertType('int', $a);
                assertType('string', $b);
            }
        