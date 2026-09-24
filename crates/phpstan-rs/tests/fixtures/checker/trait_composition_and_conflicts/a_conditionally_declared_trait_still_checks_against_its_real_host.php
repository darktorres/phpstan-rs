<?php
            if (true) {
                trait Greeter {
                    public function greet(): string { return $this->name(); }
                }
            }
            class Person {
                use Greeter;
                public function name(): string { return "Alice"; }
            }
        