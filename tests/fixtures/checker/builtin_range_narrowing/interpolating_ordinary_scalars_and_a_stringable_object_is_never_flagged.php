<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, string $s, Foo $f): string {
                return "{$i} {$s} {$f}";
            }
        