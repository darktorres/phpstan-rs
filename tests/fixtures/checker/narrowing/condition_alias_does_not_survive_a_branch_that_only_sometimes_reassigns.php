<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function unrelatedCond2(): bool { return true; }
            function other2(): Base1 { return new Base1(); }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                if (unrelatedCond2()) {
                    $x = other2();
                }
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        