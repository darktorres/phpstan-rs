<?php

// The include may throw from the included file: the catch runs and $ok is
// false afterwards, so the comparison is not always false.
function load(string $file): void
{
    $ok = true;
    try {
        include $file;
    } catch (RuntimeException $e) {
        $ok = false;
    }
    if ($ok === false) {
        echo 'failed';
    }
}
