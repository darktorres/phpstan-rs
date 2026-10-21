<?php
$x = '';
$walk = function (int $n) use (&$walk, &$x): void {
    $x = $x . 'a';
    if ($n > 0) {
        $walk($n - 1);
    }
};
$walk(rand(0, 9));
