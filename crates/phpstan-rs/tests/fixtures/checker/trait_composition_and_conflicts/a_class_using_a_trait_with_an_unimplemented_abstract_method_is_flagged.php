<?php
            trait Greetable9 {
                abstract public function getName(): string;
                public function greet(): string { return "Hello " . $this->getName(); }
            }
            class Person9 { use Greetable9; }
        