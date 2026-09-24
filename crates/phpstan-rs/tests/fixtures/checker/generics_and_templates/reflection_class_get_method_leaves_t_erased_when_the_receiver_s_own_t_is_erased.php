<?php
            use function PHPStan\Testing\assertType;
            function f(string $className): void {
                $reflClass = new ReflectionClass($className);
                assertType('ReflectionMethod<mixed>', $reflClass->getMethod('bar'));
            }
        