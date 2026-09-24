<?php
            /** @param list<int> $arr */
            function f(array $arr): void {
                $n = count($arr);
                for ($i = 0; $i < $n; $i++) {
                    $arr[$i] = 0;
                }
                needsList($arr);
            }
            /** @param list<int> $x */
            function needsList(array $x): void {}
        