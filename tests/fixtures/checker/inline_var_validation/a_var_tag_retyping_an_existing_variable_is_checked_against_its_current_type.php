<?php
            /** @return int */
            function g() { return 1; }
            function f(int $n): void {
                $m = g();
                $k = g();
                /** @var string $n */
                echo $n;
                /** @var string $m */
                echo $m;
                /** @var int<0, max> $k */
                echo $k;
            }
        