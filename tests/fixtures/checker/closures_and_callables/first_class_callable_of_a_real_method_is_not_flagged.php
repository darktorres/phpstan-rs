<?php
            class Foo { public function bar(): void {} }
            function test(Foo $foo): void {
                $g = $foo->bar(...);
            }
        