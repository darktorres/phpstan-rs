<?php
            /** @deprecated */
            function oldFunc(): void {}
            class Foo {
                /** @deprecated */
                public function bar(): void {
                    oldFunc();
                }
            }
        