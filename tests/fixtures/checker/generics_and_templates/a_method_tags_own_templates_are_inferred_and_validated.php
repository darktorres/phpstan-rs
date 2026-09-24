<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            /**
             * @template T
             * @method U wrap<U>(U $x)
             * @method static list<V> many<V of object>(V ...$xs)
             * @method T shadow<T>(T $x)
             * @method void bad<Foo>()
             * @method void opt<A = int, B>(A $a, B $b)
             */
            class Box {
                public function __call(string $n, array $a): mixed { return null; }
                public static function __callStatic(string $n, array $a): mixed { return null; }
            }
            /** @param Box<int> $b */
            function f(Box $b): void {
                assertType('5', $b->wrap(5));
                assertType('list<Foo>', Box::many(new Foo()));
            }
        