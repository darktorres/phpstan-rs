<?php
class Foo { public Foo $bar; public int $baz = 0; public function m(): Foo { return $this; } }
function test(?Foo $f): void {
    echo $f?->bar->baz;
    echo $f?->m()->baz;
    echo $f?->bar->m()->baz;
}
class Arr { /** @var array{k: Foo} */ public array $a = ['k' => new Foo()]; }
function test2(?Arr $x, ?Foo $g): void {
    echo $x?->a['k']->baz;
    echo ($g?->bar)->baz;
    $p = 'bar';
    echo $g?->$p->baz;
    echo $g->bar->baz;
}
function test3(?Foo $f): void {
    \PHPStan\Testing\assertType('int|null', $f?->bar->baz);
    \PHPStan\Testing\assertType('Foo|null', $f?->m());
}
