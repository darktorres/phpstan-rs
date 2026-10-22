<?php
function take(int $i): int {
    return $i;
}

function f(int $n): int {
    return take($n + 1) + take($n * 2);
}

echo f(3), "\n";
