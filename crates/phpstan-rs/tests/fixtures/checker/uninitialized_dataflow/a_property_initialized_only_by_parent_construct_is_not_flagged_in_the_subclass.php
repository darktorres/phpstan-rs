<?php
            class Base {
                protected int $x;
                public function __construct() {
                    $this->x = 1;
                }
            }
            class Child extends Base {
                protected int $x;
                public function __construct() {
                    parent::__construct();
                    echo $this->x;
                }
            }
        