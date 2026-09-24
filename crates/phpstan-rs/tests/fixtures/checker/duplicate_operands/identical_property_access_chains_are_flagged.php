<?php
            class C { public int $x = 1; }
            function f(C $obj): bool {
                return $obj->x === $obj->x;
            }
        