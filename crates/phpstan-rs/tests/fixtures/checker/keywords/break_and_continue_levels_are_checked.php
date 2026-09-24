<?php
            function f(array $xs): void {
                break;
                foreach ($xs as $x) {
                    break 2;
                    switch ($x) {
                        case 1:
                            continue;
                        case 2:
                            continue 2;
                    }
                    break 0;
                    $c = function () { continue; };
                }
            }
        