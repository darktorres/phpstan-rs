<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->init();
                }
                private function init(): void {
                    echo $this->x;
                    $this->x = 1;
                }
            }
        