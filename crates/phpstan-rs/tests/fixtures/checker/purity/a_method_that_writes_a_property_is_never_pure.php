<?php
            final class Foo {
                public int $x = 0;
                public function setX(int $v): void {
                    $this->x = $v;
                }
            }
            $f = new Foo();
            $f->setX(1);
        