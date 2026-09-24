<?php
            /** @return list<int> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    foreach (['1', '2'] as $s) {
                        $x[] = $s;
                    }
                }
                return array_map(intval(...), $x);
            }
        