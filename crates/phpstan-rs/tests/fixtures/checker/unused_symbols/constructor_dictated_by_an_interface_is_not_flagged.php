<?php
            interface Makeable {
                public function __construct(int $seed);
            }
            abstract class Base implements Makeable {}
            class Direct implements Makeable {
                public function __construct(int $seed) {
                    echo 1;
                }
            }
            class Inherited extends Base {
                public function __construct(int $seed) {
                    echo 1;
                }
            }
        