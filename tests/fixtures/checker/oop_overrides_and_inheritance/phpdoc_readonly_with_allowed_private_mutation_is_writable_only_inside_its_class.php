<?php
            class Counter {
                /** @phpstan-readonly-allow-private-mutation */
                public int $count = 0;
                /** @readonly */
                public int $strict;
                public function __construct() { $this->strict = 1; }
                public function bump(): void {
                    $this->count++;
                    $this->count = 2;
                    $this->strict = 3;
                }
            }
            function outside(Counter $c): void { $c->count = 5; }
        