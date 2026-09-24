<?php
            class Foo {
                public const X = 3;
                public const int Y = 4;
                public int $x = self::X;
                public int $y = self::Y;
                public string $bad = self::X;
            }
        