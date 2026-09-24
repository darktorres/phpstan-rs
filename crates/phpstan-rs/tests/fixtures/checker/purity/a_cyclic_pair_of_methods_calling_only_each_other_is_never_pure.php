<?php
            final class Foo {
                public function a(): void {
                    $this->b();
                }
                public function b(): void {
                    $this->a();
                }
            }
            $f = new Foo();
            $f->a();
        