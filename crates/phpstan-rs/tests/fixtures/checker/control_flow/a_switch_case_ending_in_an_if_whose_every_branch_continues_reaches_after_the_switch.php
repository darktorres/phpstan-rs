<?php
            function f(int $x, bool $c): string {
                switch ($x) {
                    case 1:
                        $onlyFromCase1 = "a";
                        if ($c) {
                            continue;
                        } else {
                            continue;
                        }
                    default:
                        return "other";
                }
                return $onlyFromCase1;
            }
        