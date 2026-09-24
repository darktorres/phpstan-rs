<?php
            final class Foo {
                public function noop(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $f->noop();
        