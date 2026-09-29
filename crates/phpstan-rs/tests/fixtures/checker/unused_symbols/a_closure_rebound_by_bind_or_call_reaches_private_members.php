<?php
namespace App;
use Closure as C;
final class Foo { private function secret(): int { return 1; } }
final class Bar { private function hidden(): int { return 1; } }
final class Baz { private function unused(): int { return 1; } }
function f(): int {
    $fn = function (): int { return $this->secret(); };
    $g = function (): int { return $this->hidden(); };
    C::BIND($g, new Bar(), Bar::class);
    return $fn->call(new Foo());
}
