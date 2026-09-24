<?php
            class A { public int $x = 0; }
            class B {}
            function f(A|B $obj): int { return $obj->x; }
        