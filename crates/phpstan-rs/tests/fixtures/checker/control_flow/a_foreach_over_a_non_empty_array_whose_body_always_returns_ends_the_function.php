<?php
function f(): int {
    foreach ([1, 2] as $x) {
        return $x;
    }
}

function g(array $items): int {
    foreach ($items as $x) {
        return 1;
    }
}
