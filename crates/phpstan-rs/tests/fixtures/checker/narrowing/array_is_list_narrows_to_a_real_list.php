<?php
            /** @param list<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (array_is_list($x)) {
                    sink($x);
                }
            }
        