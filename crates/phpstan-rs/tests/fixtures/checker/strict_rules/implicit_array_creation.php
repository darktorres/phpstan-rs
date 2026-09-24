<?php
function f(bool $b, array $rows): void {
    $fresh[] = 1;
    $defined = [];
    $defined[] = 1;
    if ($b) { $maybe = []; }
    $maybe['k'] = 1;
    foreach ($rows as $row) {
        $acc[] = $row;
    }
    $_SESSION['x'] = 1;
    $nested['a']['b'] = 1;
    $fn = function () { $inner[] = 1; };
}
