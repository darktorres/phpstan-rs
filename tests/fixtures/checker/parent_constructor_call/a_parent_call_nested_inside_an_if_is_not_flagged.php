<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct(bool $flag) {
                    if ($flag) {
                        parent::__construct();
                    } else {
                        parent::__construct();
                    }
                }
            }
        