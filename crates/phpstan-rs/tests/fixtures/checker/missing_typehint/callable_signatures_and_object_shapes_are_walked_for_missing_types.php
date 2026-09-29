<?php
/** @template T */
final class Coll {}
/**
 * @param callable(array): void $a
 * @param object{a: array} $b
 * @param callable(Coll): void $c
 * @param object{c: callable} $d
 */
function f(callable $a, object $b, callable $c, object $d): void {}
