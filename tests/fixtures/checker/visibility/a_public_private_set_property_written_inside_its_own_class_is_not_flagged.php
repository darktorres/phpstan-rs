<?php
            class Foo {
                public private(set) int $x;
                public function setX(int $x): void {
                    $this->x = $x;
                }
            }
        