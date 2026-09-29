<?php
use function PHPStan\Testing\assertType;
/** @template T */
final class Box { /** @param T $v */ public function __construct(public mixed $v) {} }
/** @template T of int */
final class IBox { /** @param T $v */ public function __construct(public int $v) {} }
/** @template T of array-key */
final class KBox { /** @param T $v */ public function __construct(public int|string $v) {} }
/**
 * @template T
 * @param T $x
 * @return T
 */
function identity(mixed $x): mixed { return $x; }
assertType('Box<int>', new Box(5));
assertType('Box<string>', new Box('a'));
assertType('IBox<5>', new IBox(5));
assertType('KBox<string>', new KBox('a'));
assertType('5', identity(5));
assertType('Box<list<int>>', new Box([1, 2]));
