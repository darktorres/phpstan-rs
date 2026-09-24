<?php
            class Base {
                protected function helper(): int { return 1; }
            }
            class Child extends Base {
                public function reveal(): int {
                    return $this->helper();
                }
            }
        