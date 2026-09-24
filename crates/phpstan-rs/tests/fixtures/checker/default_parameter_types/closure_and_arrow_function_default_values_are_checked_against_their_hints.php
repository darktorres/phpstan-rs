<?php
            class Holder {
                const NAME = 'n';
                public function make(): void {
                    $a = function (int $x = 'str', ?int $y = null, int $z = null, $u = 1) {};
                    $b = fn (string $s = self::NAME, bool $flag = 0) => 1;
                }
            }
        