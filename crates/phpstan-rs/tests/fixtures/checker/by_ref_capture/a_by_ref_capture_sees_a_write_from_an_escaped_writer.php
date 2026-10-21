<?php
class Holder {
    public mixed $w = null;
}
$h = new Holder();
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
$h->w = function () use (&$x): void {
    $x = 'str';
};
($h->w)();
$f();
