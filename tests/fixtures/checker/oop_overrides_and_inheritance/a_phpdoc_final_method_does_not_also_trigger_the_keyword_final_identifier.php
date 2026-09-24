<?php
            class Base {
                /** @final */
                public function locked(): int { return 1; }
            }
            class Sub extends Base {
                public function locked(): int { return 2; }
            }
        