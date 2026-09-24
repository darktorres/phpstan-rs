<?php
            trait Greetable {
                public function greet(): string { return "hi"; }
            }
            class C {
                use Greetable;
            }
        