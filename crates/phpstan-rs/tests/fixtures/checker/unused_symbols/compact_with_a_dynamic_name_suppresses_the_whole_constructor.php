<?php
            class Bag {
                public function __construct(int $x, int $y, string $which) {
                    $this->store(compact($which));
                }
                private function store(array $a): void {}
            }
        