<?php
            class Foo {
                public function niceMethod(): void {}
            }
            function f(Foo $foo): void {
                $foo->niceMethod();
            }
        