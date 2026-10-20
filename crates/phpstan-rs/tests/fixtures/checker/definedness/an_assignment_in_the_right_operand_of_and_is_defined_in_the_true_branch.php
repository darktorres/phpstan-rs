<?php
function f(string $glob, int $i): void {
    for ($i = 0; $i < 3; $i++) {
        if ('[' === $glob && false !== ($end = strpos($glob, ']', $i))) {
            echo $end;
        }
    }
}
