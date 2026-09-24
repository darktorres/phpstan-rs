<?php
            class Base {
                public function __construct(public int $x) {
                }
            }
            final class Sub extends Base {
                public function __construct(int $x) {
                    parent::__construct($x);
                }
            }
        