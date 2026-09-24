<?php
            class Widget15 { public int $value = 0; }
            /** @param null $x */
            function f($x): int {
                if ($x instanceof Widget15) {
                    return $x->value;
                }
                return 0;
            }
        