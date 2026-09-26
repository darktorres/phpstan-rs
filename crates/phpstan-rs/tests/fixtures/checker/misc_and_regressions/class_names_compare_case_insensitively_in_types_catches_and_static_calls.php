<?php
class Foo { public static function make(): self { return new self(); } public function name(): string { return 'f'; } }
class Bar extends foo {}
interface Runs {}
class R implements runs {}
function f(): foo { return FOO::make(); }
function g(foo $x): string { return $x->name(); }
function h(): string { return g(new Bar()) . g(new Foo()); }
function i(object $o): bool { return $o instanceof foo; }
function j(runs $r): void {}
function k(): void { j(new R()); }
function m(): void {
    try { throw new \RuntimeException('x'); } catch (\runtimeexception $e) { echo $e->getMessage(); }
}
function n(closure $c): void { $c(); }
function o(\traversable $t): void { foreach ($t as $v) { echo $v; } }
function p(): void { n(fn() => 1); o(new \ArrayIterator([])); }
