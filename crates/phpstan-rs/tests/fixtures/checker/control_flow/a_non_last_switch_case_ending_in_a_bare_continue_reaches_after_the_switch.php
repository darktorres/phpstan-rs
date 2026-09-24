<?php
            function f(int $x): string {
                switch ($x) {
                    case 1:
                        $onlyFromCase1 = "one";
                        continue;
                    default:
                        return "other";
                }
                return $onlyFromCase1;
            }
        