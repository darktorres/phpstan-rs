<?php
            trait Inner {
                public function greet(): string { return $this->name(); }
            }
            trait Outer {
                use Inner;
            }
            class Concrete {
                use Outer;
                public function name(): string { return "x"; }
            }
        