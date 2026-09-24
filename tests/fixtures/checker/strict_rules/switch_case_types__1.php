<?php
function f(int $n, string $s, mixed $m): void {
    switch ($n) {
        case 1: break;
        case 'one': break;
        case null: break;
        default: break;
    }
    switch ($s) { case 'a': case 2: break; }
    switch ($m) { case 'a': case 2: break; }
    switch (true) { case $n > 1: break; }
}
