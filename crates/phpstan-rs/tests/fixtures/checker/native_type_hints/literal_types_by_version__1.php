<?php
namespace App;
final class Box {}
/** @return false|null */
function a() { return null; }
/** @return true */
function b() { return true; }
/** @return Box */
function c() { return new Box(); }
final class M {
    /** @param string $name */
    public function __get($name) { return 1; }
    /** @param int $x */
    public function __invoke($x) {}
    /** @param int $x */
    public function __custom($x) {}
}
