<?php
            class C {
                #[NoDiscard]
                static function m(): int { return 1; }
            }
            C::m();
        