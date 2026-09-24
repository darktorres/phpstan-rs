<?php
            class C {}
            function f(C $c): bool {
                return isset($c->missing);
            }
        