<?php
            /** @deprecated */
            class OldFoo {
                public function bar(): void {}
            }
            function f(OldFoo $foo): void {
                $foo->bar();
            }
        