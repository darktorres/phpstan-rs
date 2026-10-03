<?php
function g(): void {
    goto a;
    echo 1;
    a:
    echo 2;
}

function h(bool $c): int {
    if ($c) {
        goto out;
    }
    return 1;
    out:
    return 2;
}
