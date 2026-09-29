<?php
function test(int $x): int {
    $f = function (int &$v): int { return $v; };
    return $x |> $f;
}
