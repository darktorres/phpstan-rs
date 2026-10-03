<?php
function f(int $x, bool $c): int {
    switch ($x) {
        case 1:
            if ($c) {
                break;
            }
            return 1;
        default:
            return 2;
    }
}
