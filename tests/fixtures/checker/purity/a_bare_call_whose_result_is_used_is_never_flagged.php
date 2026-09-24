<?php
            final class Foo {
                public function pure(): int {
                    return 1;
                }
            }
            $f = new Foo();
            $x = $f->pure();
            echo $x;
        