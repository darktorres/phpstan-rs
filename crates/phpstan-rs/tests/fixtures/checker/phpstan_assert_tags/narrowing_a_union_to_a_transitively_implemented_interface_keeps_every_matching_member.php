<?php
            interface Marker {}
            class Base {
                /** @phpstan-assert-if-true Marker $this */
                public function isMarked(): bool { return true; }
            }
            abstract class Mid extends Base implements Marker {}
            class LeafA extends Mid { public int $x = 1; }
            class LeafB extends Mid { public int $x = 1; }
            function run(LeafA|LeafB $v): void {
                if ($v->isMarked()) {
                    echo $v->x;
                }
            }
        