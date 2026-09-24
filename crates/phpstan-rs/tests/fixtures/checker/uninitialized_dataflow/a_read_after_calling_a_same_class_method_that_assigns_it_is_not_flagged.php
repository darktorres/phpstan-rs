<?php
            class Foo {
                public int $x;
                public function __construct() {
                    $this->init();
                    echo $this->x;
                }
                private function init(): void {
                    $this->x = 1;
                }
            }
        