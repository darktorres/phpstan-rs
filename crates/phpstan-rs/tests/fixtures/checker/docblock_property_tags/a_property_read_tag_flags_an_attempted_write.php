<?php
            /**
             * @property-read int $x
             */
            class A {}
            function f(A $a): void { $a->x = 1; }
        