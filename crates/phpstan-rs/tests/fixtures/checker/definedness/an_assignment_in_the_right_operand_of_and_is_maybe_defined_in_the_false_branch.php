<?php
function f(?string $c): void {
    if (is_string($c) && false !== $i = strpos($c, '::')) {
        echo 1;
    } else {
        echo $i;
    }
    echo $i;
}
