<?php
class Base { public static function make(): static { return new static(); } public function run(): void {} }
class Child extends Base {
    public function go(?Child $other): void {
        $this->make();
        $this->run();
        static::make();
        $other?->make();
        $f = $this->make(...);
    }
}
