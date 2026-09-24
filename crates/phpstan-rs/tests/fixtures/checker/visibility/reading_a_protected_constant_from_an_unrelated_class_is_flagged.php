<?php
            class Foo {
                protected const X = 1;
            }
            function test(): int {
                return Foo::X;
            }
        