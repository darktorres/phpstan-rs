<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->a();
                }
                private function a(): void {
                    $this->b();
                }
                private function b(): void {
                    $this->a();
                    $this->x = 1;
                }
            }
        