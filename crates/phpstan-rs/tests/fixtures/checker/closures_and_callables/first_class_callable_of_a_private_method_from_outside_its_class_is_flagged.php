<?php
            class Foo {
                private function secret(): void {}
            }
            function test(Foo $foo): void {
                $g = $foo->secret(...);
            }
        