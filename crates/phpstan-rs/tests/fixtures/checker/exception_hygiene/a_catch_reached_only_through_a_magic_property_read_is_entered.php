<?php

class Lazy
{
    public function __get(string $name): mixed
    {
        throw new RuntimeException('no property');
    }
}

// The read calls __get, which throws: the catch runs and $ok is false
// afterwards, so the comparison is not always false.
function read(Lazy $lazy): void
{
    $ok = true;
    try {
        $value = $lazy->foo;
    } catch (RuntimeException $e) {
        $ok = false;
    }
    if ($ok === false) {
        echo 'failed';
    }
}
