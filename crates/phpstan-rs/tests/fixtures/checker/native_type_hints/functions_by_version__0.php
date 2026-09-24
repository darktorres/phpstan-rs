<?php
/**
 * @param int $a
 * @param list<string> $b
 * @param int|string $c
 * @param int $d
 * @param resource $e
 * @param positive-int $f
 * @return void
 */
function f($a, $b, $c, $d = null, $e = null, $f = 1) {}
/** @return int|false */
function g() { return 1; }
/** @return mixed */
function h() { return 1; }
/** @param int $x */
function typed(int $x): void {}
/** @param int $x */
function badDefault($x = 'a'): void {}
