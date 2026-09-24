<?php
        interface A {} interface B {}
        class Foo {
            const int|string C = 1;
            public ?int $a = null;
            public int|string|null $b = null;
            public function m(?Foo $x, int|false $y, (A&B)|null $z, A&B $w, mixed $v): static|null { return null; }
            public function n(): void {}
            public function o(): never { throw new \Exception(); }
            public function p(): ?self { return null; }
        }
        $f = fn(int|string $x): int|string => $x;
        $g = function (?int $x): ?int { return $x; };
        