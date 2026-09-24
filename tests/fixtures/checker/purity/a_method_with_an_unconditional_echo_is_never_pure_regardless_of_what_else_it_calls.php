<?php
            final class Foo {
                public function outer(): void {
                    $this->inner();
                    echo "hi";
                }
                public function inner(): void {
                }
            }
            $f = new Foo();
            $f->outer();
        