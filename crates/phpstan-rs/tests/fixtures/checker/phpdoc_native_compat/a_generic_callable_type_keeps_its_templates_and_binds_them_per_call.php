<?php
            namespace App;
            use function PHPStan\Testing\assertType;
            class Foo {}
            /**
             * @param callable<T>(T): T $f
             * @param \Closure<T of Foo>(T): list<T> $g
             * @param pure-callable(int): string $h
             */
            function f(callable $f, \Closure $g, callable $h): void {
                assertType('callable<T>(T): T', $f);
                assertType('Closure<T of App\Foo>(T): list<T>', $g);
                assertType('callable(int): string', $h);
                assertType('5', $f(5));
                assertType('list<App\Foo>', $g(new Foo()));
            }
        