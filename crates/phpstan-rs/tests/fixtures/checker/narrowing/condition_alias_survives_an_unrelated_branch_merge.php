<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function unrelatedCond1(): bool { return true; }
            function doSomething1(): void {}
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                if (unrelatedCond1()) {
                    doSomething1();
                }
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        