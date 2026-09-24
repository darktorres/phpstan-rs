<?php
            /** @deprecated */
            class OldFoo {
                public int $x = 1;
            }
            function f(OldFoo $foo): void {
                echo $foo->x;
            }
        