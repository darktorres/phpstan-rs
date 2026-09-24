<?php
            class C {
                #[NoDiscard]
                function m(): int { return 1; }
            }
            function test(C $c) {
                $c->m();
            }
        