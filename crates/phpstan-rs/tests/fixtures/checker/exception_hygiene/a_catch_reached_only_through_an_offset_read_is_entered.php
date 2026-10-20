<?php

// The read calls offsetGet on an ArrayAccess object, which may throw: the
// catch runs and $ok is false afterwards, so the comparison is not always
// false.
function offset_read(mixed $bag): void
{
    $ok = true;
    try {
        $value = $bag['x'];
    } catch (RuntimeException $e) {
        $ok = false;
    }
    if ($ok === false) {
        echo 'failed';
    }
}
