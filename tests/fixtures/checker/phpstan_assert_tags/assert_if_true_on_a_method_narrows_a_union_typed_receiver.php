<?php
            class Base {
                /** @phpstan-assert-if-true A|B $this */
                public function isAB(): bool { return true; }
            }
            class A extends Base { public int $x = 1; }
            class B extends Base { public int $x = 1; }
            class C extends Base {}
            function run(A|B|C $v): void {
                if ($v->isAB()) {
                    echo $v->x;
                }
            }
        