<?php
            class Value {
                private function __construct(private int $x) {}
                private function __clone() {}
                public static function of(int $x): self {
                    return new self($x);
                }
            }
        