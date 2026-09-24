<?php
class Open {}
class Extended {}
class Child extends Extended {}
final class Sealed {
    protected int $count = 0;
    protected function helper(): void {}
    protected const LIMIT = 1;
}
abstract class Base {
    public function run(): void {}
    public function hook(): void {}
    abstract public function step(): void;
    final public function done(): void {}
    private function internal(): void {}
}
final class Impl extends Base {
    public function hook(): void {}
    public function step(): void {}
}
/** @ORM\Entity */
class Mapped {}
