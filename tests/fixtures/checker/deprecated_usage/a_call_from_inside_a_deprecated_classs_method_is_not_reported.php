<?php
            /** @deprecated */
            function oldFunc(): void {}
            /** @deprecated */
            class Foo {
                public function bar(): void {
                    oldFunc();
                }
            }
        