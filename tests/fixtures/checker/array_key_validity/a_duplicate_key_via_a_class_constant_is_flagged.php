<?php
            class C { const KEY = 'a'; }
            function f(): array {
                return [C::KEY => 1, 'a' => 2];
            }
        