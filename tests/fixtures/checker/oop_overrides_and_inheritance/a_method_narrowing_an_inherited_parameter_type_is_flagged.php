<?php
            class Animal2 {}
            class Dog2 extends Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class DogOnlyHandler2 extends Handler2 { public function handle(Dog2 $a): void {} }
        