<?php
            class Foo {
                private const X = 1;
            }
            function test(): int {
                return Foo::X;
            }
        