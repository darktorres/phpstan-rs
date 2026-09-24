<?php
            class C {
                public function __serialize(): array {
                    return ['a' => 1, 'b' => 'x'];
                }
            }
        