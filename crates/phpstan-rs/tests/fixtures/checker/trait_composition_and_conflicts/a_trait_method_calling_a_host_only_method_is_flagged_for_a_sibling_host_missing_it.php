<?php
            trait Greeter {
                public function greet(): string { return $this->name(); }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
            class Robot {
                use Greeter;
            }
        