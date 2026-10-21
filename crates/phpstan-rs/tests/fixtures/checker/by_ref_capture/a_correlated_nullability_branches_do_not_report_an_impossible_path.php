<?php
function correlatedNull(?string $y): void {
    $x = null;
    if ($y !== null) {
        $x = 'str';
    }
    if ($y !== null) {
        strlen($x);
    }
}
