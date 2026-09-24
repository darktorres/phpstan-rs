<?php
            class Foo {}
            /** @template T */
            class Box {
                /** @var callable<T>(T): T */
                public $f;
                /**
                 * @template U
                 * @param callable<U>(U): U $a
                 * @param callable<Foo>(Foo): void $b
                 * @param callable<V = int, W>(V): W $c
                 * @param callable<X of callable>(X): X $d
                 * @return Closure<Y of int = string>(Y): Y
                 */
                public function m(callable $a, callable $b, callable $c, callable $d): Closure { return fn($y) => $y; }
            }
            /** @template T
             *  @param callable<T>(T): T $a */
            function g(callable $a): void {}
            /** @template T of Foo */
            function h(): void {}
            /** @template Foo */
            function k(): void {}
        