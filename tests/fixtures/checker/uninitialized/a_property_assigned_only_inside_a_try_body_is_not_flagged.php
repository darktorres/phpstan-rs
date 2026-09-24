<?php
            class Foo {
                public int $x;
                public function __construct() {
                    try {
                        $this->x = riskyCompute();
                    } catch (\Exception $e) {
                    }
                }
            }
            function riskyCompute(): int { return 1; }
        