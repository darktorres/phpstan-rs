<?php
            /** @param list<mixed> $x */
            function sink($x): void {}
            function f(mixed $x): void {
                if (is_array($x) && array_is_list($x)) {
                    sink($x);
                }
            }
        