<?php
            namespace App;
            use Closure;
            enum Suit: string { case Hearts = 'h'; case Spades = 's'; }
            /** @phpstan-type Element array{type: string} */
            class Known {
                const A_ONE = 1;
                const A_TWO = 2;
                /**
                 * @param array{a: int}['a'] $a
                 * @param Closure(int): string $b
                 * @param self::A_* $c
                 * @param Suit::Hearts $d
                 * @param string[] $e
                 * @param int<1, 5> $f
                 * @param non-empty-array<int, string> $g
                 * @param list<int>&non-empty-array $h
                 * @param \Closure(int): void $i
                 * @param static $j
                 * @param Known::class $k
                 * @param closed-resource|open-resource $l
                 * @param \OCI-Lob $m
                 * @param Element&array{type: 'a'} $n
                 * @return array<int, string>
                 */
                public function f($a, $b, $c, $d, $e, $f, $g, $h, $i, $j, $k, $l, $m, $n) { return []; }
                /** @psalm-return never-return */
                public function g() { throw new \Exception(); }
                /** @return no-return */
                public function h() { throw new \Exception(); }
            }
        