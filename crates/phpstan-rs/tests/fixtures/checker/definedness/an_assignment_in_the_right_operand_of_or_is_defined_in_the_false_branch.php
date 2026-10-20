<?php
function f(string $glob, int $i): void {
    for ($i = 0; $i < 3; $i++) {
        if ('[' !== $glob || !($end = strpos($glob, ']', $i))) {
            continue;
        }
        echo $end;
    }
}
