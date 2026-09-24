<?php
            class C { public int $x = 1; }
            function f(C $c): bool {
                return isset($c->x);
            }
        