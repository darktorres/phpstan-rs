<?php
            class Foo {
                public function run(): int {
                    $this->helper();
                    return 1;
                }
                private function helper(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $f->run();
        