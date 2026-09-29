<?php
            use function PHPStan\Testing\assertType;
            function f(string $className): void {
                $reflClass = new ReflectionClass($className);
                assertType('ReflectionMethod<object>', $reflClass->getMethod('bar'));
            }
        