<?php
function conjunction(bool $a, bool $b): void {
    $x = null;
    if ($a && $b) {
        $x = 'str';
    }
    if ($a) {
        strlen($x);
    }
}
