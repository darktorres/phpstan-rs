<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(string $s, Foo $f): void {
                $s . $f;
            }
        