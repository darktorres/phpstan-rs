<?php
            use function PHPStan\Testing\assertType;
            assertType('ReflectionClass', new ReflectionClass('NotARealClass'));
        