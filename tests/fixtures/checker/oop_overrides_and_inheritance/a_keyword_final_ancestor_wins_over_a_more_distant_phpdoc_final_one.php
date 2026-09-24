<?php
            class Grandparent {
                /** @final */
                public function locked(): int { return 1; }
            }
            class Base extends Grandparent {
                final public function locked(): int { return 1; }
            }
            class Sub extends Base {
                public function locked(): int { return 2; }
            }
        