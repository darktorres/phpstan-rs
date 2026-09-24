<?php
            class Foo {
                /** @deprecated */
                public function oldMethod(): void {}
            }
            function f(Foo $foo): void {
                $foo->oldMethod();
            }
        