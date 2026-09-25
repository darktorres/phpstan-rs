<?php
class Foo { public static function bar(): int { return 1; } }
function f(string $m): void {
    Foo::$m();
    Foo::{$m}();
}
