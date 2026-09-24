<?php
            class Foo {
                public const X = 1;
                const Y = 2;
            }
            function test(): int {
                return Foo::X + Foo::Y;
            }
        