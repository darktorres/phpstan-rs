<?php
            trait Greets {
                private function greeting(): string { return 'hi'; }
                public function greet(): string { return $this->greeting(); }
            }
            class Greeter {
                use Greets;
            }
        