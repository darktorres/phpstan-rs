<?php
            class Foo {
                /** @deprecated */
                public function bar(): void {}
            }
            function f(Foo $foo): void {
                $foo->bar();
            }
        