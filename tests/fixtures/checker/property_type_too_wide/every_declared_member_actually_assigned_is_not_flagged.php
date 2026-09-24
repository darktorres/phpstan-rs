<?php
            class C {
                private int|string $x;
                public function __construct(bool $b) {
                    $this->x = $b ? 1 : 'y';
                }
            }
        