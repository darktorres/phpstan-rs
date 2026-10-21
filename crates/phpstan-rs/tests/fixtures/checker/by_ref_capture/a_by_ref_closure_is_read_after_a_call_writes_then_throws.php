<?php
$x = null;
$f = function () use (&$x): void {
    $x = 'str';
    throw new \Exception('e');
};
try {
    $f();
} catch (\Exception $e) {
}
strlen($x);
