<?php
            class Money {
                public function __construct(private readonly int $cents) {}
                public function inDollars(): float {
                    return $this->cents / 100.0;
                }
            }
        