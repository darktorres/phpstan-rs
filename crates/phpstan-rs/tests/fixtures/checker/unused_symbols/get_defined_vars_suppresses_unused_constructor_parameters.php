<?php
            class Bag {
                public function __construct(int $x, int $y) {
                    $this->store(get_defined_vars());
                }
                private function store(array $a): void {}
            }
        