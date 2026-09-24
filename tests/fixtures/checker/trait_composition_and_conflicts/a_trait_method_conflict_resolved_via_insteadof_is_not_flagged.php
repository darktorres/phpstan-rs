<?php
            trait T4a { public function bar(): int { return 1; } }
            trait T4b { public function bar(): int { return 2; } }
            class C4 {
                use T4a, T4b {
                    T4a::bar insteadof T4b;
                }
            }
        