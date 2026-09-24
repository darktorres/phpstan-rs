<?php
            class Base {
                protected int $x = 0;
                public function __construct() {
                    $this->setup();
                }
                protected function setup(): void {
                }
            }
            class Child extends Base {
                protected int $x;
                public function __construct() {
                    parent::__construct();
                    echo $this->x;
                }
                protected function setup(): void {
                    $this->x = 1;
                }
            }
        