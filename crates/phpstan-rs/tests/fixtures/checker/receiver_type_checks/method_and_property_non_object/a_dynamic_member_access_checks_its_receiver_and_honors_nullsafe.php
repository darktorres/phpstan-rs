<?php
final class Foo { public int $a = 1; public function m(): int { return 1; } }
function f(?Foo $x, string $p, int $i, mixed $m): void {
    echo $x->$p;
    echo $x?->$p;
    echo $x->{'m'}();
    echo $x?->{'m'}();
    echo $i->$p;
    echo $m->$p;
}
