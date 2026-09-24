<?php
            class Base { public function __construct() {} }
            class Child extends Base {
                public function __construct() {
                    parent::__construct();
                }
            }
        