<?php
            enum Kind { case A; }
            class Foo { const BAR = 1; }
            /** @phpstan-pure */
            function readConst(): int {
                $k = Kind::A;
                return Foo::BAR;
            }
        