<?php
function f(array $rows): int {
    $vals = $rows;
    $vals[0] = &$stub;
    return count($vals);
}
function g(): void {
    $alias = &$created;
    $created = 1;
    echo $alias;
}
