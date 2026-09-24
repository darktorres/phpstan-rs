<?php
            final class Foo {
                public function a(): int {
                    return $this->b();
                }
                public function b(): int {
                    return $this->c();
                }
                public function c(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $f->a();
        