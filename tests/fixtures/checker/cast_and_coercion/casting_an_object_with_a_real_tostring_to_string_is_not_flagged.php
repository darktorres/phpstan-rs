<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            function test(Foo $f): string {
                return (string) $f;
            }
        