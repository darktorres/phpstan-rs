<?php
            /** @implements ArrayAccess<int, string> */
            class Bag implements ArrayAccess {
                public function offsetExists(mixed $o): bool { return true; }
                public function offsetGet(mixed $o): mixed { return 1; }
                public function offsetSet(mixed $offset, mixed $value): void {}
                public function offsetUnset(mixed $o): void {}
            }
            class IntBag implements ArrayAccess {
                public function offsetExists(mixed $o): bool { return true; }
                public function offsetGet(mixed $o): mixed { return 1; }
                public function offsetSet(?int $offset, string $value): void {}
                public function offsetUnset(mixed $o): void {}
            }
            function f(array $arr, string $s, IntBag $bag, Bag $any): void {
                $arr[[1]] = 1;
                $arr['ok'] = 1;
                $s[] = 'x';
                $s['a'] = 'x';
                $s[0] = 'x';
                $bag['k'] = 'v';
                $bag[1] = 5;
                $bag[] = 'fine';
                $any['whatever'] = new \stdClass();
            }
        