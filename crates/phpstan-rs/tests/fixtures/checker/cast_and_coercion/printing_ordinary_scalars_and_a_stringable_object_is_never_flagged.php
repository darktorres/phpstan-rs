<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(int $i, Foo $f): void {
                print $i;
                print $f;
            }
        