<?php
function correlated(bool $a): void {
    $x = null;
    if ($a) {
        $x = 'str';
    }
    if ($a) {
        strlen($x);
    }
}
