<?php
            class Foo {
                /** @deprecated */
                public function oldMethod(): void {}
            }
            function test(Foo $foo): void {
                $g = $foo->oldMethod(...);
            }
        