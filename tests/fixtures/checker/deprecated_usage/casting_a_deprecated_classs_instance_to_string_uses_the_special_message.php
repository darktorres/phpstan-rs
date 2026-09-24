<?php
            class Foo {
                /** @deprecated */
                public function __toString(): string { return "x"; }
            }
            function f(Foo $foo): string {
                return (string) $foo;
            }
        