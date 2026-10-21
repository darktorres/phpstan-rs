<?php
class M {
    public function __get(string $name): int {
        global $x;
        $x = 'str';
        return 1;
    }
}
$o = new M();
$x = null;
$f = function () use (&$x, $o): void {
    $o->foo;
    strlen($x);
};
$f();
