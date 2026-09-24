<?php
            class Animal {}
            class Cat extends Animal { public function meow(): string { return "meow"; } }
            function f(Animal $animal): string {
                if ($animal instanceof Cat) {
                    return $animal->meow();
                }
                return "";
            }
        