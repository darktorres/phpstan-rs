<?php
            /** @internal */
            class Thing {
                public function doStuff(): void {}
            }
            class Other {
                public function f(Thing $t) {
                    $t->doStuff();
                }
            }
        