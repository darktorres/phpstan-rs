<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, string $s, Foo $f): void {
                echo $i, $s, $f;
            }
        