<?php
            use function PHPStan\Testing\assertType;
            function f(int $base): void {
                assertType('int', strtotime('-30 days', $base));
            }
        