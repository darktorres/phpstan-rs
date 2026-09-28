<?php
namespace App;

interface I {}
final class C implements I {}
final class Inv { public function __invoke(string $s): int { return 1; } }
final class St { public static function s(): void {} public function i(): void {} }

/** @param array{a: int} $x */
function needShape(array $x): void {}
/** @param array{a: int, ...} $x */
function needOpenShape(array $x): void {}
/** @param object{a: int} $x */
function needObjShape(object $x): void {}
function needInt(int $x): void {}
function needCallable(callable $c): void {}
/** @param callable(int): int $c */
function needIntCallable(callable $c): void {}
/** @param interface-string $s */
function needIface(string $s): void {}
/** @param \NoSuchKlass $x */
function needUnknown($x): void {}

/**
 * @template T
 * @param T $t
 * @param array{a?: int} $opt
 * @param object{a?: int} $optObj
 * @param class-string<I> $ci
 * @param class-string<C> $cc
 * @param class-string<I|C> $cu
 */
function f($t, array $opt, object $optObj, string $ci, string $cc, string $cu, object $o): void {
    needShape($opt);
    needShape(['a' => 1, 'b' => 2]);
    needOpenShape(['a' => 1, 'b' => 2]);
    needObjShape($optObj);
    needInt($t);
    needCallable('nosuchfunc_xyz');
    needCallable('strlen');
    needCallable([1, 2]);
    needCallable([new St(), 'i']);
    needCallable([new St(), 'nope']);
    needCallable([St::class, 's']);
    needCallable([St::class, 'i']);
    needCallable('App\St::s');
    needCallable(new Inv());
    needIntCallable(new Inv());
    needIface($ci);
    needIface($cc);
    needIface($cu);
    needUnknown(5);
    needUnknown($o);
}
