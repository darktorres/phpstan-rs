<?php
function f(?string $c): void {
    $x = match (true) {
        is_string($c) && false !== $i = strpos($c, '::') => $i,
        default => null,
    };
}
