<?php
            final class Foo {
                public function outer(): int {
                    return $this->inner();
                }
                public function inner(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $f->outer();
        