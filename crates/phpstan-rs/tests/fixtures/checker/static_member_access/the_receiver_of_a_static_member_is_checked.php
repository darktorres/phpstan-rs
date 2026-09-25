<?php
class Foo { const A = 1; public static int $n = 0; public static function m(): void {} }
function f(): void {
    echo $undefined1::A;
    echo $undefined2::$n;
    echo $undefined3::class;
    $undefined4::m();
}
