<?php
            class Foo {
                private const X = 1;
                public function getX(): int {
                    return self::X;
                }
            }
        