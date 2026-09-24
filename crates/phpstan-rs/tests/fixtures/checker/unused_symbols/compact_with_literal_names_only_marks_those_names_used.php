<?php
            class Bag {
                public array $data = [];
                public function __construct(int $x, int $y) {
                    $this->data = compact('x');
                }
            }
        