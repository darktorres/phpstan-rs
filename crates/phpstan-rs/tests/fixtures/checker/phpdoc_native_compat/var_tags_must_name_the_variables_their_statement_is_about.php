<?php
            function f(array $items): void {
                /** @var int $wrong */
                $x = g();
                /** @var int $x */
                $y = g();
                /**
                 * @var int
                 * @var string
                 */
                $z = g();
                /** @var int */
                [$a, $b] = [1, 2];
                /** @var string $nope */
                foreach ($items as $k => $v) {}
                /** @var int $ghost */
                echo 1;
                /** @var int */
                echo 2;
                /** @var int $x */
                echo 3;
                /** @var int $$w */
                $w = g();
            }
            function g(): mixed { return 1; }
        