<?php
            use function PHPStan\Testing\assertType;
            function f(\GMP $g): void {
                assertType('GMP', -$g);
                assertType('GMP', +$g);
                assertType('GMP', ~$g);
            }
        