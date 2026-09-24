<?php
            class Magic { public function __get(string $n): mixed { return 1; } }
            class Stringy { public function __toString(): string { return ''; } }
            /** @implements ArrayAccess<string, int> */
            abstract class Bag implements ArrayAccess {}
            function test(Magic $m, Stringy $s, Bag $b, mixed $x, object $o): void {
                $m->anything;
                $b['key'];
                (string) $s;
                'a' . $s;
                "{$s}";
                $x->prop;
                $x['k'];
                $o->prop;
            }
        