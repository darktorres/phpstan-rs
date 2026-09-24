<?php
            class Foo {
                public private(set) int $x = 1;
                public function setX(int $x): void {
                    $this->x = $x;
                }
            }
        