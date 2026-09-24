<?php
            /** @deprecated */
            class OldFoo {
                /** @deprecated */
                public function bar(): void {}
            }
            function f(OldFoo $foo): void {
                $foo->bar();
            }
        