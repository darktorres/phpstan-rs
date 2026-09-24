<?php
            class Foo {
                public int $x;
                public function __construct() {
                }
                public function handle(int $value): void {
                    $this->x = $value;
                }
            }
        