<?php
function f(): int {
    try {
        return 1;
        echo 'unreachable';
    } catch (\Exception $e) {
        return 2;
    }
}
