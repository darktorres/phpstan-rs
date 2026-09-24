<?php
            /** @return list<int> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    $x[] = 1;
                }
                sort($x);
                return $x;
            }
        