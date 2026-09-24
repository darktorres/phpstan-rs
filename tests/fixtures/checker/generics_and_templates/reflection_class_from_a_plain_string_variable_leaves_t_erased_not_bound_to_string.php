<?php
            use function PHPStan\Testing\assertType;
            function f(string $className): void {
                assertType('ReflectionClass', new ReflectionClass($className));
            }
        