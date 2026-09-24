<?php
            /** @return list<string> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    foreach (['a', 'b'] as $s) {
                        $x[] = $s;
                    }
                }
                sort($x);
                return $x;
            }
        