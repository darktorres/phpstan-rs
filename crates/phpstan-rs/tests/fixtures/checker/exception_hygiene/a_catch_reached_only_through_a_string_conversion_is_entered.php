<?php

class Named
{
    public function __toString(): string
    {
        throw new RuntimeException('no name');
    }
}

// The concatenation calls __toString, which throws: the catch runs and $ok is
// false afterwards, so the comparison is not always false.
function convert(Named $named): void
{
    $ok = true;
    try {
        $text = 'a' . $named;
    } catch (RuntimeException $e) {
        $ok = false;
    }
    if ($ok === false) {
        echo 'failed';
    }
}
