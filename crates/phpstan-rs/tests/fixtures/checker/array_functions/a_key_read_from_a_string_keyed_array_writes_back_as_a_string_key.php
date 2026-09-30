<?php
use function PHPStan\Testing\assertType;

/** @param array<string, int> $in */
function copies(array $in): void {
    $out = [];
    foreach ($in as $k => $v) {
        assertType('non-decimal-int-string', $k);
        $out[$k] = $v;
    }
    assertType('array<string, int>', $out);
    foreach (array_keys($in) as $k) {
        assertType('non-decimal-int-string', $k);
    }
}

function takes_string(string $s): void {}

/** @param array<int|string, int> $in */
function passes(array $in): void {
    foreach ($in as $k => $v) {
        if (is_string($k)) {
            takes_string($k);
        }
    }
}

/** @param list<string> $l */
function flips(array $l): void {
    assertType('array<int|string, int<0, max>>', array_flip($l));
}

/** @param array<mixed> $a */
function searches(array $a, mixed $needle): void {
    assertType('int|string|false', array_search($needle, $a, true));
}

function builds(int $id, int $n, mixed $x): void {
    $a = [];
    $a['user_' . $id] = 1;
    assertType('non-empty-array<non-falsy-non-decimal-int-string, 1>', $a);
    $b = [];
    $b[sprintf('%s-%s', $id, $n)] = 1;
    assertType('non-empty-array<string, 1>', $b);
    $c = [];
    $c['-' . $n] = 1;
    assertType('non-empty-array<int|string, 1>', $c);
    $d = [];
    $d[md5((string) $x)] = 1;
    assertType('non-empty-array<string, 1>', $d);
    $e = [];
    $e[md5((string) $x, true)] = 1;
    assertType('non-empty-array<int|string, 1>', $e);
    if (!is_numeric($x) && is_string($x)) {
        assertType('non-decimal-int-string', $x);
    }
}

copies(['a' => 1, 'b' => 2]);
passes(['a' => 1, 2 => 3]);
flips(['a', 'b']);
searches(['a' => 1], 1);
builds(5, 7, 'q');
