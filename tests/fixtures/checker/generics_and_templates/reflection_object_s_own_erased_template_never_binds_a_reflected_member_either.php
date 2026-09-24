<?php
            use function PHPStan\Testing\assertType;
            function f(object $x): void {
                $reflClass = new ReflectionObject($x);
                assertType('ReflectionMethod<mixed>', $reflClass->getMethod('bar'));
            }
        