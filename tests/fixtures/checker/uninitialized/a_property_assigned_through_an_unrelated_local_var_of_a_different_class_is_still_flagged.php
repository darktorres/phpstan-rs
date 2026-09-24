<?php
            class Other {
                public int $x = 0;
            }
            class Node {
                public int $x;
                public function f(): void {
                    $other = new Other();
                    $other->x = 5;
                }
            }
        