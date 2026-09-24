<?php
            class C { const KEY = 0; }
            function f(): array {
                return [C::KEY => 'x', 'y'];
            }
        