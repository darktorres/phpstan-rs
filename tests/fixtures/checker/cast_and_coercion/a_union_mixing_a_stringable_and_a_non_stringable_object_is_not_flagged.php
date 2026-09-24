<?php
            class Foo {
                public function __toString(): string { return "foo"; }
            }
            class Bar {}
            function test(Foo|Bar $x): string {
                return (string) $x;
            }
        