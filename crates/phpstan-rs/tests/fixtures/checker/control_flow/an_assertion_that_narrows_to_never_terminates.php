<?php
final class Foo {}
final class Bar {}

function f(Foo $x): int {
    assert($x instanceof Bar);
    echo 'unreachable';
}
