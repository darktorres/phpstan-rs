<?php
            class Base {
                /** @throws \LogicException */
                public function __construct() {}
            }
            class Child extends Base {
                /** @throws \RuntimeException */
                public function __construct() {}
            }
        