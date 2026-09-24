<?php
            function test(int $mask): void {
                /** @var \Some\Unresolvable\IntMask<1> $mask */
                $mask | 1;
            }
        