<?php
            interface Shape {
                public function area(): int;
            }
            class Square implements Shape {
                public function __construct(int $side) {
                    echo 1;
                }
                public function area(): int { return 1; }
            }
        