<?php
            final class Foo {
                public function a(): int {
                    return $this->b();
                }
                public function b(): int {
                    return $this->a();
                }
            }
            $f = new Foo();
            $f->a();
