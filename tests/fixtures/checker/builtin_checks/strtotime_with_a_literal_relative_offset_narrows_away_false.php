<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('int', strtotime('-1 hour'));
                assertType('int', strtotime('-90 days'));
                assertType('int', strtotime('+3 weeks ago'));
                assertType('int', strtotime('2 Days'));
            }
        