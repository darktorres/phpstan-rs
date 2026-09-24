<?php
            class Widget {
                private int $count;
                public function __construct(int $count) {
                    $this->count = $count;
                }
                public function describe(): \Closure {
                    return function (): int {
                        return $this->count;
                    };
                }
            }
        