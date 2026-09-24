<?php
            /** @deprecated */
            function oldFunc(): void {}
            class Foo {
                public function bar(): void {
                    oldFunc();
                }
            }
        