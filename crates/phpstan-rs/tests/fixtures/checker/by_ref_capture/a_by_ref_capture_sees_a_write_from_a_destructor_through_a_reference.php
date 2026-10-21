<?php
class D {
    public mixed $r = null;
    public function __destruct() {
        $this->r = 'str';
    }
}
$x = null;
$f = function () use (&$x): void {
    strlen($x);
};
$d = new D();
$d->r = &$x;
$d = null;
$f();
