<?php
use function PHPStan\Testing\assertType;

class Holder {
    /** @var array<mixed> */
    public array $m = [];

    public function run(string $s): void {
        if (preg_match('/(\d+)/', $s, $this->m) === 1) {
            assertType('array{0: string, 1: numeric-string}', $this->m);
        }
    }
}

function f(string $s): void {
    $r = preg_match('/(a)(b)?/', $s, $m);
    if ($r === 1) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    }
    if ($r !== 0) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}|array{}", $m);
    }
    if ($r > 0) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    }
    if ($r >= 1) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    }
    if ($r == true) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    }
    if ($r === false) {
        assertType('array{}', $m);
    }
    if ($r == 0) {
        assertType('array{}', $m);
    }
    if ($r) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    } else {
        assertType('array{}', $m);
    }
    if (1 === $r) {
        assertType("array{0: string, 1: 'a', 2?: 'b'}", $m);
    }
}

function rebound(string $s): void {
    $r = preg_match('/(a)/', $s, $m);
    $m = ['x'];
    if ($r === 1) {
        assertType("array{0: 'x'}", $m);
    }
}

function named(string $s): void {
    if (preg_match(subject: $s, pattern: '/(a)/', matches: $m, flags: PREG_OFFSET_CAPTURE) === 1) {
        assertType("array{0: array{0: string, 1: int}, 1: array{0: 'a', 1: int}}", $m);
    }
}

function all(string $s): void {
    if (preg_match_all('/(a)/', $s, $m) === 2) {
        assertType("array{0: array{0: string, 1: string}, 1: array{0: 'a', 1: 'a'}}", $m);
    }
    if (preg_match_all('/(a)/', $s, $m, PREG_SET_ORDER) > 0) {
        assertType("non-empty-list<array{0: string, 1: 'a'}>", $m);
    }
    if (preg_match_all('/(a)/', $s, $m) === 0) {
        assertType('array{0: array{}, 1: array{}}', $m);
    }
}

function subjects(string $s, string $t, string $u): void {
    if (preg_match('/^\d+$/D', $s)) {
        assertType('non-empty-numeric-string', $s);
    }
    if (preg_match('/^\d+$/m', $t)) {
        assertType('non-empty-string', $t);
    }
    if (preg_match('/a?/', $u)) {
        assertType('string', $u);
    }
}

(new Holder())->run('x12');
f('ab');
f('zz');
rebound('a');
named('a');
all('aa');
all('zz');
subjects('12', "x\n3", 'q');
