<?php
            /** @param list<int> $arr */
            function f(array $arr): void {
                for ($i = 0; count($arr) > $i; $i++) {
                    $arr[$i] = 0;
                }
                needsList($arr);
            }
            /** @param list<int> $x */
            function needsList(array $x): void {}
        