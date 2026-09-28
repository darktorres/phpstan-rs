<?php
use function PHPStan\Testing\assertType;
class Expr {}
class Dim extends Expr {}
interface CallLike {}
class MethodCall implements CallLike {}
function loop(Expr $foo, Expr $bar): void {
    while ($foo instanceof Dim) {
        assertType('Dim', $foo);
        $foo = $bar;
    }
}
function iface(CallLike $node): void {
    if ($node instanceof MethodCall) {
        assertType('MethodCall', $node);
    }
}
