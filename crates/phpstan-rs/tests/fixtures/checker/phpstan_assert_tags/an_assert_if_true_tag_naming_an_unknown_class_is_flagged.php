<?php
            class Foo {
                /** @phpstan-assert-if-true NoSuchClass $x */
                public function check($x): bool { return true; }
            }
        