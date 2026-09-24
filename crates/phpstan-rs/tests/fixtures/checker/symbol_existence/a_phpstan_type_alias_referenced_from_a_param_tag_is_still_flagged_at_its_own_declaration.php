<?php
            class Foo {}
            /** @phpstan-type MyAlias foo */
            class Bar {
                /** @param MyAlias $x */
                public function f($x): void {}
            }
        