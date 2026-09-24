<?php
            class Foo {
                public protected(set) int $x = 1;
            }
            class Bar extends Foo {
                public function setX(int $x): void {
                    $this->x = $x;
                }
            }
        