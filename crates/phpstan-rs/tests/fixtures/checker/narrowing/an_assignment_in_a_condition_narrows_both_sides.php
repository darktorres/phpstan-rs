<?php
use function PHPStan\Testing\assertType;

class Foo {}
class Node {
    public ?Node $next = null;
}

/** @param list<int> $x */
function counts(array $x): void {
    if (($n = count($x)) > 0) {
        assertType('int<1, max>', $n);
        assertType('non-empty-list<int>', $x);
    }
}

function preg(string $s): void {
    if (($r = preg_match('/(a)/', $s, $m)) === 1) {
        assertType("array{0: string, 1: 'a'}", $m);
    }
    if ($r2 = preg_match('/(b)/', $s, $m2)) {
        assertType("array{0: string, 1: 'b'}", $m2);
        assertType('1', $r2);
    }
}

function position(string $s): void {
    if (!$pos = strpos($s, 'x')) {
        assertType('false|0', $pos);
    }
}

function instance(mixed $x): void {
    if ($o = $x instanceof Foo) {
        assertType('Foo', $x);
        assertType('true', $o);
    }
}

/** @param resource $fh */
function lines($fh): void {
    while (($line = fgets($fh)) !== false) {
        assertType('string', $line);
    }
}

function walk(Node $node): void {
    if ($node = $node->next) {
        assertType('Node', $node);
    }
}

/** @param array{x: array{x: int}} $a */
function self_reference(array $a): void {
    if ($a = $a['x']) {
        assertType('array{x: int}', $a);
    }
    assertType('array{x: int}', $a);
}

counts([1, 2]);
counts([]);
preg('ab');
preg('zz');
position('abc');
position('xbc');
instance(new Foo());
instance(1);
$fh = fopen('php://memory', 'r+');
if ($fh === false) {
    exit(1);
}
fwrite($fh, "a\nb\n");
rewind($fh);
lines($fh);
$n = new Node();
$n->next = new Node();
walk($n);
walk(new Node());
self_reference(['x' => ['x' => 1]]);
