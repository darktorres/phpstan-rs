<?php
            class Foo {
                public function __construct(public readonly int $x) {}
                public function setX(int $x): void { $this->x = $x; }
            }
            class Bar extends Foo {
                public function __construct(int $x) {
                    parent::__construct($x);
                    $this->x = $x;
                }
            }
        