<?php
            /**
             * @property string $x
             */
            class A { public int $x = 0; }
            function f(A $a): int { return $a->x; }
        