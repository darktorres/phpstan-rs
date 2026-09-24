<?php
            class Unrelated {}
            class Node {
                public function eq(Unrelated $u, Node $n, object $o): void {
                    if ($u instanceof static) { echo 1; }
                    if ($n instanceof static) { echo 2; }
                    if ($o instanceof static) { echo 3; }
                }
            }
            final class Leaf {
                public function eq(Leaf $l): void {
                    if ($l instanceof static) { echo 4; }
                }
            }
        