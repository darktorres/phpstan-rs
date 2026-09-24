<?php
            use function PHPStan\Testing\assertType;
            /** @template T */
            class Bag {
                /**
                 * @template N
                 * @param N $item
                 * @phpstan-self-out self<T|N>
                 */
                public function add($item): void {}
                /** @phpstan-this-out static */
                public function reset(): void {}
            }
            /** @param Bag<int> $bag */
            function f(Bag $bag): void {
                $bag->add('x');
                assertType("Bag<int|'x'>", $bag);
            }
        