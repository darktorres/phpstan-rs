<?php
class Foo { public function foo(): void {} }
function f(bool $c): void {
    if (isset($never)) { $never->foo(); }
    echo $never;
}
