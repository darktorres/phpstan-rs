<?php
/** @param array<string, int> $a */
function g(array $a): bool
{
    var_dump(
        1,
        2,
    );
    unset($a["k"],);
    return isset($a["j"],);
}
