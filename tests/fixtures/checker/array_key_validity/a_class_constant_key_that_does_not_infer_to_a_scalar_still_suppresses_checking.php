<?php
            class C { const KEY = self::class; }
            function f(): array {
                return ['a' => 1, C::KEY => 2, 'a' => 3];
            }
        