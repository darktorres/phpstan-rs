<?php
/** @param int<0, 0> $z */
function f(int $z): float {
    return 1 % $z + 1 / 0.0;
}
