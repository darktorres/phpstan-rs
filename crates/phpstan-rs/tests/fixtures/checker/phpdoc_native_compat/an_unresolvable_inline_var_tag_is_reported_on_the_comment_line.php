<?php
            function f(): void {
                /**
                 * @var string[optional] $x
                 */
                $x = g();
                /** @var int<3, 1> */
                $y = g();
                /** @var list<int> $z */
                $z = g();
            }
            function g(): mixed { return 1; }
        