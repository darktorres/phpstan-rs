<?php
/** @template T */
final class Box {}
/**
 * @param Box<NoSuchA> $a
 * @param callable(NoSuchB): void $b
 * @param array<NoSuchC> $c
 * @param NoSuchD $d
 * @param \Closure(int): NoSuchE $e
 */
function f($a, $b, $c, $d, $e): void {}
