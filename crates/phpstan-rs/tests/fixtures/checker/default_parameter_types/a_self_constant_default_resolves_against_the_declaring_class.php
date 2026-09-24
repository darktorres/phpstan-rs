<?php
            interface IO {
                public const NORMAL = 2;
                public function write(string $m, int $v = self::NORMAL): void;
            }
            class C {
                const X = 1;
                const NAME = 'n';
                public function f(int $a = self::X, int $b = self::NAME): void {}
            }
        